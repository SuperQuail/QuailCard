//! 把单个远端媒体映射为短命回环地址，ffmpeg 可按需 seek 而不下载整片。

use std::{io, sync::Arc, time::Duration};

use reqwest::header::{ACCEPT_RANGES, CONTENT_LENGTH, CONTENT_RANGE};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{TcpListener, TcpStream},
    task::{JoinHandle, JoinSet},
    time::timeout,
};

use crate::{
    error::CommandError,
    video::bilibili::http::{BiliClient, CookieJar, REFERER},
};

const HEADER_LIMIT: usize = 8 * 1024;
const CONNECTION_LIMIT: usize = 4;
const WRITE_CHUNK: usize = 64 * 1024;
const HEADER_TIMEOUT: Duration = Duration::from_secs(5);
const IO_TIMEOUT: Duration = Duration::from_secs(30);

/// 持有代理的生命周期；不允许后台任务脱离抽帧操作继续下载。
pub(crate) struct MediaProxy {
    url: String,
    task: JoinHandle<()>,
}

impl MediaProxy {
    /// 只向子进程提供回环令牌地址，不暴露远端签名或认证信息。
    pub(crate) fn url(&self) -> String {
        self.url.clone()
    }
}

impl Drop for MediaProxy {
    /// 中止监听任务会析构其 JoinSet，连带中止所有连接及上游请求。
    fn drop(&mut self) {
        self.task.abort();
    }
}

/// 为一个固定远端 URL 建立无凭据代理，绑定完成后才返回可用地址。
pub(crate) async fn start(url: String) -> Result<MediaProxy, CommandError> {
    let client = BiliClient::new(CookieJar::default())?;
    let listener = TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0))
        .await
        .map_err(|_| CommandError::new("VIDEO_PROXY_ERROR", "无法启动媒体回环代理"))?;
    let address = listener
        .local_addr()
        .map_err(|_| CommandError::new("VIDEO_PROXY_ERROR", "无法获取媒体回环地址"))?;
    let path = format!("/{}", uuid::Uuid::now_v7());
    let local_url = format!("http://{address}{path}");
    let task = tokio::spawn(serve(listener, client, Arc::new(url), Arc::new(path)));
    Ok(MediaProxy {
        url: local_url,
        task,
    })
}

/// 满额时停止 accept；JoinSet 同时限制活跃任务与待回收结果的数量。
async fn serve(listener: TcpListener, client: BiliClient, url: Arc<String>, path: Arc<String>) {
    let mut connections = JoinSet::new();
    loop {
        tokio::select! {
            _ = connections.join_next(), if !connections.is_empty() => {}
            accepted = listener.accept(), if connections.len() < CONNECTION_LIMIT => {
                let Ok((socket, peer)) = accepted else { break };
                if !peer.ip().is_loopback() { continue; }
                let (client, url, path) = (client.clone(), url.clone(), path.clone());
                connections.spawn(async move {
                    // 每个连接只服务一次请求；断开或超时直接丢弃上游响应。
                    let _ = handle(socket, &client, &url, &path).await;
                });
            }
        }
    }
    // 包括监听错误退出在内，析构都必须取消子任务。
}

struct Request {
    head: bool,
    range: Option<String>,
}

/// 先完整校验有界请求头，再接触远端；不转发客户端提供的任意请求头。
async fn handle(
    mut socket: TcpStream,
    client: &BiliClient,
    url: &str,
    path: &str,
) -> io::Result<()> {
    let request = match timeout(HEADER_TIMEOUT, read_request(&mut socket, path)).await {
        Ok(Ok(request)) => request,
        Ok(Err(status)) => return reject(&mut socket, status).await,
        Err(_) => return reject(&mut socket, "408 Request Timeout").await,
    };
    let mut response = match timeout(
        IO_TIMEOUT,
        client.stream_range(url, REFERER, request.range.as_deref()),
    )
    .await
    {
        Ok(Ok(response)) if matches!(response.status().as_u16(), 200 | 206) => response,
        _ => return reject(&mut socket, "502 Bad Gateway").await,
    };
    let status = if response.status().as_u16() == 206 {
        "206 Partial Content"
    } else {
        "200 OK"
    };
    let mut headers = format!("HTTP/1.1 {status}\r\nConnection: close\r\n");
    for name in [CONTENT_LENGTH, CONTENT_RANGE, ACCEPT_RANGES] {
        if let Some(value) = response.headers().get(&name) {
            let Ok(value) = value.to_str() else {
                return reject(&mut socket, "502 Bad Gateway").await;
            };
            // 上游头也设上限；绝不把 Location、Set-Cookie 等头带到回环端。
            if !value.bytes().all(|byte| (32..=126).contains(&byte))
                || headers.len() + name.as_str().len() + value.len() + 6 > HEADER_LIMIT
            {
                return reject(&mut socket, "502 Bad Gateway").await;
            }
            headers.push_str(name.as_str());
            headers.push_str(": ");
            headers.push_str(value);
            headers.push_str("\r\n");
        }
    }
    headers.push_str("\r\n");
    write_bytes(&mut socket, headers.as_bytes()).await?;
    if !request.head {
        // 不收集完整响应，只保留一个传输块；分段写入并等待背压。
        while let Some(chunk) = timeout(IO_TIMEOUT, response.chunk())
            .await
            .map_err(|_| io::Error::from(io::ErrorKind::TimedOut))?
            .map_err(|_| io::Error::from(io::ErrorKind::ConnectionAborted))?
        {
            for bytes in chunk.chunks(WRITE_CHUNK) {
                write_bytes(&mut socket, bytes).await?;
            }
        }
    }
    // HEAD 与 GET 共享源站头语义，但不会消费任何响应体。
    Ok(())
}

/// 固定缓冲区包含终止符；慢速请求由调用方的整体头超时约束。
async fn read_request(socket: &mut TcpStream, path: &str) -> Result<Request, &'static str> {
    let mut buffer = [0u8; HEADER_LIMIT];
    let mut used = 0;
    loop {
        if used == buffer.len() {
            return Err("431 Request Header Fields Too Large");
        }
        let count = socket
            .read(&mut buffer[used..])
            .await
            .map_err(|_| "400 Bad Request")?;
        if count == 0 {
            return Err("400 Bad Request");
        }
        used += count;
        if let Some(end) = buffer[..used]
            .windows(4)
            .position(|part| part == b"\r\n\r\n")
        {
            let text = std::str::from_utf8(&buffer[..end]).map_err(|_| "400 Bad Request")?;
            return parse_request(text, path);
        }
    }
}

/// 严格匹配原始路径，拒绝查询、重复 Range、请求体和折叠头，避免解析歧义。
fn parse_request(text: &str, path: &str) -> Result<Request, &'static str> {
    let mut lines = text.split("\r\n");
    let mut first = lines.next().ok_or("400 Bad Request")?.split(' ');
    let method = first.next().ok_or("400 Bad Request")?;
    let target = first.next().ok_or("400 Bad Request")?;
    let version = first.next().ok_or("400 Bad Request")?;
    if first.next().is_some() || !matches!(version, "HTTP/1.0" | "HTTP/1.1") {
        return Err("400 Bad Request");
    }
    if target != path {
        return Err("404 Not Found");
    }
    if !matches!(method, "GET" | "HEAD") {
        return Err("405 Method Not Allowed");
    }
    let mut range = None;
    for line in lines {
        let (name, value) = line.split_once(':').ok_or("400 Bad Request")?;
        if name.is_empty()
            || !name.bytes().all(header_name_byte)
            || !value
                .bytes()
                .all(|byte| byte == b'\t' || (32..=126).contains(&byte))
        {
            return Err("400 Bad Request");
        }
        let value = value.trim_matches([' ', '\t']);
        if name.eq_ignore_ascii_case("range") {
            if range.is_some() || !valid_range(value) {
                return Err("400 Bad Request");
            }
            range = Some(value.to_owned());
        }
        if name.eq_ignore_ascii_case("transfer-encoding")
            || (name.eq_ignore_ascii_case("content-length") && value != "0")
        {
            return Err("400 Bad Request");
        }
    }
    Ok(Request {
        head: method == "HEAD",
        range,
    })
}

/// HTTP 字段名只允许 token 字符，防止空白与控制字符造成头边界分歧。
fn header_name_byte(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || b"!#$%&'*+-.^_`|~".contains(&byte)
}

/// 仅接受单个字节区间；限制到 u64，拒绝空区间、逆序和零长度后缀。
fn valid_range(value: &str) -> bool {
    let Some(value) = value.strip_prefix("bytes=") else {
        return false;
    };
    let Some((start, end)) = value.split_once('-') else {
        return false;
    };
    if start.is_empty() {
        return decimal(end).is_some_and(|end| end > 0);
    }
    let Some(start) = decimal(start) else {
        return false;
    };
    end.is_empty() || decimal(end).is_some_and(|end| end >= start)
}

/// 显式排除加号及非 ASCII 数字，保持 Range 语法与源站一致。
fn decimal(value: &str) -> Option<u64> {
    if value.is_empty() || !value.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    value.parse().ok()
}

/// 每次写入有独立超时，阻塞读端不能永久占用有限连接槽位。
async fn write_bytes(socket: &mut TcpStream, bytes: &[u8]) -> io::Result<()> {
    timeout(IO_TIMEOUT, socket.write_all(bytes))
        .await
        .map_err(|_| io::Error::from(io::ErrorKind::TimedOut))?
}

/// 失败只发送固定状态和空响应，不泄露上游地址、响应内容或错误详情。
async fn reject(socket: &mut TcpStream, status: &str) -> io::Result<()> {
    let response = format!("HTTP/1.1 {status}\r\nConnection: close\r\nContent-Length: 0\r\n\r\n");
    write_bytes(socket, response.as_bytes()).await
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 令牌按原始路径匹配，查询和近似路径不能获得代理访问权。
    #[test]
    fn exact_path_and_methods() {
        for target in ["/other", "/token?x=1", "/token/", "/%74oken"] {
            let request = format!("GET {target} HTTP/1.1");
            assert_eq!(
                parse_request(&request, "/token").err(),
                Some("404 Not Found")
            );
        }
        assert_eq!(
            parse_request("POST /token HTTP/1.1", "/token").err(),
            Some("405 Method Not Allowed")
        );
        assert!(parse_request("GET /token HTTP/2", "/token").is_err());
    }

    /// GET 和 HEAD 保留单区间语义，允许无 Range 的媒体探测。
    #[test]
    fn accepts_single_ranges() {
        for method in ["GET", "HEAD"] {
            let plain = parse_request(&format!("{method} /token HTTP/1.1"), "/token").unwrap();
            assert_eq!(plain.head, method == "HEAD");
            assert!(plain.range.is_none());
            for range in ["bytes=0-0", "bytes=12-99", "bytes=20-", "bytes=-30"] {
                let raw = format!("{method} /token HTTP/1.1\r\nrAnGe: {range}");
                let parsed = parse_request(&raw, "/token").unwrap();
                assert_eq!(parsed.head, method == "HEAD");
                assert_eq!(parsed.range.as_deref(), Some(range));
            }
        }
    }

    /// 禁止多区间、溢出和有歧义的头，不能把请求体解释为后续请求。
    #[test]
    fn rejects_ambiguous_headers_and_ranges() {
        for range in [
            "bytes=0-1,3-4",
            "bytes=-",
            "bytes=-0",
            "bytes=9-2",
            "bytes=+1-2",
            "bytes=1--2",
            "bytes=18446744073709551616-",
            "items=0-1",
        ] {
            let raw = format!("GET /token HTTP/1.1\r\nRange: {range}");
            assert!(parse_request(&raw, "/token").is_err(), "{range}");
        }
        for headers in [
            "Range: bytes=0-\r\nRange: bytes=1-",
            "Content-Length: 1",
            "Transfer-Encoding: chunked",
            "Range: bytes=0-\r\n folded",
            " Range: bytes=0-",
        ] {
            let raw = format!("GET /token HTTP/1.1\r\n{headers}");
            assert!(parse_request(&raw, "/token").is_err(), "{headers}");
        }
    }

    /// 仅连接回环且不发送合法请求；Drop 必须关闭监听器及未完成请求连接。
    #[tokio::test]
    async fn drop_closes_listener_and_connections() {
        timeout(Duration::from_secs(3), async {
            let proxy = start("https://example.invalid/media".to_owned())
                .await
                .unwrap();
            let url = reqwest::Url::parse(&proxy.url()).unwrap();
            assert_eq!(url.host_str(), Some("127.0.0.1"));
            let address = format!("127.0.0.1:{}", url.port().unwrap());
            let mut socket = TcpStream::connect(&address).await.unwrap();
            // 不完整头让连接停在读取阶段，绝不请求上游。
            socket.write_all(b"GET ").await.unwrap();
            drop(proxy);
            let mut byte = [0u8; 1];
            match socket.read(&mut byte).await {
                Ok(size) => assert_eq!(size, 0),
                Err(error) => assert!(matches!(
                    error.kind(),
                    io::ErrorKind::ConnectionReset | io::ErrorKind::ConnectionAborted
                )),
            }
            assert!(TcpStream::connect(&address).await.is_err());
        })
        .await
        .expect("proxy drop did not close sockets");
    }
}
