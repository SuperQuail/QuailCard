use std::{
    path::{Path, PathBuf},
    time::Duration,
};

use base64::{engine::general_purpose, Engine as _};
use futures_util::{SinkExt, StreamExt};
use sha2::{Digest, Sha256};
use tokio_tungstenite::{connect_async, tungstenite::Message};

use crate::error::CommandError;

/// Edge TTS WebSocket 端点前缀，使用固定客户端令牌鉴权。
const EDGE_TTS_BASE_URL: &str =
    "wss://speech.platform.bing.com/consumer/speech/synthesize/readaloud/edge/v1";
/// Edge TTS 固定客户端令牌，参与反爬令牌计算。
const TRUSTED_CLIENT_TOKEN: &str = "6A5AA1D4EAFF4E9FB37E23D68491D6F4";
/// Windows 时间纪元相对 Unix 纪元的偏移秒数。
const WIN_EPOCH_SECONDS: u64 = 11_644_473_600;
/// 默认美音语音名称。
const DEFAULT_VOICE: &str = "en-US-AriaNeural";
/// 对齐 edge-tts 使用的 Chromium 版本标识。
const CHROMIUM_FULL_VERSION: &str = "143.0.3650.75";
/// 连接与合成整体超时。
const SYNTHESIS_TIMEOUT: Duration = Duration::from_secs(15);
/// 单词文本的最大字符数。
const MAX_SPEECH_TEXT_CHARS: usize = 200;

/// 提供 Edge TTS 在线合成与本地缓存能力的朗读服务。
pub struct SpeechService {
    cache_dir: PathBuf,
}

impl SpeechService {
    /// 创建使用指定缓存目录的朗读服务。
    pub fn new(cache_dir: PathBuf) -> Self {
        Self { cache_dir }
    }

    /// 合成单词发音，优先返回本地缓存，未命中则在线合成并写入缓存。
    pub async fn synthesize(&self, text: &str) -> Result<String, CommandError> {
        let text = text.trim();
        if text.is_empty() || text.chars().count() > MAX_SPEECH_TEXT_CHARS {
            return Err(CommandError::validation("朗读文本长度必须为 1-200 个字符"));
        }
        let cache_file = self.cache_file(text);
        if cache_file.exists() {
            return read_audio_data_url(&cache_file);
        }
        let audio = synthesize_edge_tts(text).await?;
        std::fs::create_dir_all(&self.cache_dir).map_err(io_error)?;
        std::fs::write(&cache_file, &audio).map_err(io_error)?;
        encode_audio_data_url(&audio)
    }

    /// 计算单词语音的缓存文件名（语音与文本共同决定）。
    fn cache_file(&self, text: &str) -> PathBuf {
        let mut hasher = Sha256::new();
        hasher.update(DEFAULT_VOICE.as_bytes());
        hasher.update(text.as_bytes());
        let digest = hasher.finalize();
        self.cache_dir.join(format!("{}.mp3", hex_first(&digest)))
    }
}

/// 通过 Edge TTS WebSocket 合成音频并拼接全部音频分片。
async fn synthesize_edge_tts(text: &str) -> Result<Vec<u8>, CommandError> {
    let request = http_connect_request();
    let (mut socket, _) = tokio::time::timeout(SYNTHESIS_TIMEOUT, connect_async(request))
        .await
        .map_err(|_| tts_error("TTS_TIMEOUT", "语音合成连接超时，请检查网络"))?
        .map_err(|error| {
            tts_error(
                "TTS_CONNECT_FAILED",
                &format!("无法连接语音合成服务：{error:?}"),
            )
        })?;

    socket
        .send(Message::Text(synthesis_context_config()))
        .await
        .map_err(|_| tts_error("TTS_SEND_FAILED", "语音合成请求发送失败"))?;
    socket
        .send(Message::Text(ssml_message(text)))
        .await
        .map_err(|_| tts_error("TTS_SEND_FAILED", "语音合成请求发送失败"))?;
    let mut audio = Vec::new();
    let result = tokio::time::timeout(SYNTHESIS_TIMEOUT, async {
        while let Some(message) = socket.next().await {
            let message =
                message.map_err(|_| tts_error("TTS_STREAM_FAILED", "语音合成数据流中断"))?;
            match &message {
                Message::Text(text) => {
                    if text.contains("Path:turn.end") {
                        break;
                    }
                }
                Message::Binary(payload) => {
                    if let Some(chunk) = extract_audio_chunk(payload) {
                        audio.extend_from_slice(chunk);
                    }
                }
                Message::Close(_) => break,
                Message::Ping(_) | Message::Pong(_) | Message::Frame(_) => {}
            }
        }
        Ok::<(), CommandError>(())
    })
    .await;
    match result {
        Ok(Ok(())) if !audio.is_empty() => Ok(audio),
        Ok(Ok(())) => Err(tts_error("TTS_EMPTY", "语音合成没有返回音频数据")),
        Ok(Err(error)) => Err(error),
        Err(_) => Err(tts_error("TTS_TIMEOUT", "语音合成超时，请稍后重试")),
    }
}

/// 构造携带必要请求头的 WebSocket 握手请求（对齐 edge-tts 最新实现）。
fn http_connect_request() -> tokio_tungstenite::tungstenite::client::ClientRequestBuilder {
    use tokio_tungstenite::tungstenite::client::ClientRequestBuilder;
    let endpoint = format!(
        "{EDGE_TTS_BASE_URL}?TrustedClientToken={TRUSTED_CLIENT_TOKEN}&ConnectionId={}&Sec-MS-GEC={}&Sec-MS-GEC-Version=1-{CHROMIUM_FULL_VERSION}",
        connection_id(),
        sec_ms_gec(),
    );
    ClientRequestBuilder::new(endpoint.parse().expect("Edge TTS 端点地址无效"))
        .with_header("Pragma", "no-cache")
        .with_header("Cache-Control", "no-cache")
        .with_header("Origin", "chrome-extension://jdiccldimpdaibmpdkjnbmckianbfold")
        .with_header(
            "User-Agent",
            "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/143.0.0.0 Safari/537.36 Edg/143.0.0.0",
        )
        .with_header("Accept-Encoding", "gzip, deflate, br, zstd")
        .with_header("Accept-Language", "en-US,en;q=0.9")
        .with_header("Cookie", format!("muid={};", generate_muid()))
}

/// 生成无横线的随机连接标识。
fn connection_id() -> String {
    use rand::Rng;
    let mut rng = rand::thread_rng();
    (0..32).map(|_| rng.gen_range('0'..='9')).collect()
}

/// 生成随机的 MUID Cookie 值（32 位大写十六进制）。
fn generate_muid() -> String {
    use rand::Rng;
    const HEX_CHARS: &[u8] = b"0123456789abcdef";
    let mut rng = rand::thread_rng();
    (0..32)
        .map(|_| HEX_CHARS[rng.gen_range(0..HEX_CHARS.len())] as char)
        .collect::<String>()
        .to_uppercase()
}

/// 生成 Edge TTS 反爬验证令牌（5 分钟取整的 Windows FILETIME 与客户端令牌的 SHA256）。
fn sec_ms_gec() -> String {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    let windowed = (now + WIN_EPOCH_SECONDS) / 300 * 300;
    let ticks = windowed * 10_000_000;
    let mut hasher = Sha256::new();
    hasher.update(format!("{ticks}{TRUSTED_CLIENT_TOKEN}").as_bytes());
    let digest = hasher.finalize();
    digest.iter().map(|byte| format!("{byte:02X}")).collect()
}

/// 构造 Edge TTS 的合成上下文配置消息。
fn synthesis_context_config() -> String {
    format!(
        "X-Timestamp:{}\r\nContent-Type:application/json; charset=utf-8\r\nPath:speech.config\r\n\r\n{{\"context\":{{\"synthesis\":{{\"audio\":{{\"metadataoptions\":{{\"sentenceBoundaryEnabled\":\"false\",\"wordBoundaryEnabled\":\"false\"}},\"outputFormat\":\"audio-24khz-48kbitrate-mono-mp3\"}}}}}}}}",
        edge_timestamp()
    )
}

/// 构造包含目标语音与文本的 SSML 消息。
fn ssml_message(text: &str) -> String {
    let escaped = escape_xml(text);
    let ssml = format!(
        r#"<speak version="1.0" xmlns="http://www.w3.org/2001/10/synthesis" xmlns:mstts="https://www.w3.org/2001/mstts" xml:lang="en-US"><voice name="{DEFAULT_VOICE}">{escaped}</voice></speak>"#
    );
    format!(
        "X-RequestId:{}\r\nContent-Type:application/ssml+xml\r\nX-Timestamp:{}Z\r\nPath:ssml\r\n\r\n{}",
        connection_id(),
        edge_timestamp(),
        ssml
    )
}

/// 生成 Edge TTS 时间戳头使用的日期字符串。
fn edge_timestamp() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let seconds = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64;
    let days = seconds.div_euclid(86_400);
    let day_of_week =
        ["Sun", "Mon", "Tue", "Wed", "Thu", "Fri", "Sat"][(days + 4).rem_euclid(7) as usize];
    let (year, month, day) = civil_from_days(days);
    let months = [
        "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
    ];
    let hour = seconds.rem_euclid(86_400) / 3_600;
    let minute = seconds.rem_euclid(3_600) / 60;
    let second = seconds.rem_euclid(60);
    format!(
        "{day_of_week} {} {day:>2} {year:04} {hour:02}:{minute:02}:{second:02} GMT+0000 (Coordinated Universal Time)",
        months[(month - 1) as usize]
    )
}

/// 将自 1970-01-01 起的天数转换为公历日期（Howard Hinnant 算法）。
fn civil_from_days(days: i64) -> (i64, i64, i64) {
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let year = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = if month <= 2 { year + 1 } else { year };
    (year, month, day)
}

/// 从 Edge TTS 二进制帧中提取音频分片（前 2 字节为大端头部长度）。
fn extract_audio_chunk(payload: &[u8]) -> Option<&[u8]> {
    if payload.len() < 2 {
        return None;
    }
    let header_length = u16::from_be_bytes([payload[0], payload[1]]) as usize;
    if header_length == 0 || payload.len() < header_length + 2 {
        return None;
    }
    let header = std::str::from_utf8(&payload[2..2 + header_length]).ok()?;
    if !header
        .lines()
        .any(|line| line.trim_start().starts_with("Path:audio"))
    {
        return None;
    }
    let chunk = &payload[2 + header_length..];
    if chunk.is_empty() {
        return None;
    }
    Some(chunk)
}

/// 转义 SSML 中需要特殊处理的 XML 字符。
fn escape_xml(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}

/// 读取缓存音频并编码为浏览器可直接播放的 Data URL。
fn read_audio_data_url(path: &Path) -> Result<String, CommandError> {
    let audio = std::fs::read(path).map_err(io_error)?;
    encode_audio_data_url(&audio)
}

/// 将音频字节编码为 Data URL。
fn encode_audio_data_url(audio: &[u8]) -> Result<String, CommandError> {
    Ok(format!(
        "data:audio/mpeg;base64,{}",
        general_purpose::STANDARD.encode(audio)
    ))
}

/// 将文件系统错误转换为用户可读错误。
fn io_error(error: std::io::Error) -> CommandError {
    CommandError::provider("TTS_CACHE_FAILED", format!("语音缓存读写失败：{error}"))
}

/// 构造固定错误码的语音合成错误。
fn tts_error(code: &'static str, message: &str) -> CommandError {
    CommandError::provider(code, message)
}

/// 将摘要字节转换为十六进制字符串。
fn hex_first(digest: &[u8]) -> String {
    digest
        .iter()
        .take(16)
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    /// SSML 消息会转义 XML 特殊字符。
    fn escapes_xml_special_characters() {
        assert_eq!(
            escape_xml("a & b <c> \"d\" 'e'"),
            "a &amp; b &lt;c&gt; &quot;d&quot; &apos;e&apos;"
        );
        assert!(ssml_message("hello & goodbye").contains("hello &amp; goodbye"));
    }

    #[test]
    /// 二进制帧按 2 字节头部长度提取音频分片。
    fn extracts_audio_chunk_from_binary_frame() {
        let header = b"Path:audio\r\nContent-Type:audio/mpeg\r\n\r\n";
        let header_length = (header.len() as u16).to_be_bytes();
        let mut frame = Vec::new();
        frame.extend_from_slice(&header_length);
        frame.extend_from_slice(header);
        frame.extend_from_slice(&[0x00, 0x00, 0xff, 0xff]);
        assert_eq!(
            extract_audio_chunk(&frame),
            Some(&frame[2 + header.len()..])
        );
    }

    #[test]
    /// 非音频帧、空分片或畸形帧不参与拼接。
    fn ignores_non_audio_frames() {
        let header = b"Path:turn.start\r\n\r\n";
        let mut frame = Vec::new();
        frame.extend_from_slice(&(header.len() as u16).to_be_bytes());
        frame.extend_from_slice(header);
        frame.extend_from_slice(b"payload");
        assert_eq!(extract_audio_chunk(&frame), None);
        assert_eq!(extract_audio_chunk(b"\x00\x01"), None);
        assert_eq!(extract_audio_chunk(b""), None);
    }

    #[test]
    /// 时间戳包含标准的日期头格式。
    fn builds_edge_timestamp_header() {
        let timestamp = edge_timestamp();
        assert!(timestamp.ends_with("GMT+0000 (Coordinated Universal Time)"));
        assert!(
            timestamp.starts_with("Mon")
                || timestamp.starts_with("Tue")
                || timestamp.starts_with("Wed")
                || timestamp.starts_with("Thu")
                || timestamp.starts_with("Fri")
                || timestamp.starts_with("Sat")
                || timestamp.starts_with("Sun")
        );
        assert_eq!(civil_from_days(0), (1970, 1, 1));
        assert_eq!(civil_from_days(11_061), (2000, 4, 14));
    }

    #[test]
    /// 缓存文件名由语音与文本共同决定且长度稳定。
    fn builds_stable_cache_file_name() {
        let service = SpeechService::new(PathBuf::from("cache"));
        let first = service.cache_file("hello");
        let second = service.cache_file("hello");
        let other = service.cache_file("world");
        assert_eq!(first, second);
        assert_ne!(first, other);
        assert!(first.extension().is_some_and(|ext| ext == "mp3"));
    }

    #[test]
    /// 超长文本被拒绝，避免滥用合成接口。
    fn rejects_overlong_text() {
        let service = SpeechService::new(PathBuf::from("cache"));
        let long = "x".repeat(201);
        let error = tokio::runtime::Runtime::new()
            .expect("创建运行时失败")
            .block_on(service.synthesize(&long))
            .expect_err("超长文本不应合成");
        assert_eq!(error.code, "VALIDATION_ERROR");
    }

    #[test]
    /// Sec-MS-GEC 令牌长度固定为 64 位大写十六进制。
    fn generates_sec_ms_gec_token() {
        let token = sec_ms_gec();
        assert_eq!(token.len(), 64);
        assert!(token.chars().all(|character| character.is_ascii_hexdigit()));
    }
    #[tokio::test]
    #[ignore = "依赖 Edge TTS 在线服务，仅在开发环境手动运行"]
    /// 真实合成一个单词并返回可播放的音频 Data URL。
    async fn synthesizes_word_over_network() {
        let service =
            SpeechService::new(PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("target/tts-cache"));
        let result = service.synthesize("ephemeral").await;
        eprintln!(
            "合成结果: {:?}",
            result.as_ref().map(|value| value.chars().count())
        );
        let result = result.expect("在线合成失败");
        assert!(result.starts_with("data:audio/mpeg;base64,"));
        assert!(result.len() > 200);
        let cached = service.synthesize("ephemeral").await.expect("缓存读取失败");
        assert_eq!(result, cached);
    }
}
