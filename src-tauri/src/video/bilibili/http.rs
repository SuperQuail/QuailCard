//! B 站 HTTP 客户端：固定请求头、Cookie 拼装与会话级重试。

use reqwest::{Client, Response};
use serde_json::Value;
use std::{
    sync::{Arc, Mutex},
    time::Duration,
};
use zeroize::Zeroizing;

use crate::error::CommandError;

#[cfg(test)]
#[path = "http_tests.rs"]
mod tests;

/// 桌面浏览器 UA；B 站对空 UA 与脚本 UA 更严格。
pub(crate) const USER_AGENT: &str = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/126.0.0.0 Safari/537.36";

/// 主站来源，接口与内容 CDN 都要求。
pub(crate) const REFERER: &str = "https://www.bilibili.com/";

/// 跨域来源，字幕与媒体 CDN 缺少时会 403。
pub(crate) const ORIGIN: &str = "https://www.bilibili.com";

/// 网络故障退避间隔；明确的拒绝响应不自动重试，避免加重限制。
const RETRY_DELAYS: [u64; 3] = [1, 3, 8];

pub(crate) use super::cookies::CookieJar;

pub(crate) use super::http_errors::api_error;
use super::http_errors::{endpoint, http_error};

/// 绑定一次会话 Cookie 的 B 站客户端。
#[derive(Clone)]
pub(crate) struct BiliClient {
    http: Client,
    cookie: CookieJar,
    received: Arc<Mutex<CookieJar>>,
}

impl BiliClient {
    /// 创建带固定超时的客户端；连接失败不暴露内部细节。
    pub(crate) fn new(cookie: CookieJar) -> Result<Self, CommandError> {
        let http = Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .connect_timeout(Duration::from_secs(15))
            .timeout(Duration::from_secs(60))
            .build()
            .map_err(|_| CommandError::new("VIDEO_HTTP_ERROR", "无法初始化网络连接"))?;
        Ok(Self {
            http,
            cookie,
            received: Arc::new(Mutex::new(CookieJar::default())),
        })
    }

    /// 当前会话 Cookie。
    pub(crate) fn cookie(&self) -> &CookieJar {
        &self.cookie
    }

    /// 请求接口并返回 data 字段；业务错误码统一映射。
    pub(crate) async fn api(&self, url: &str) -> Result<Value, CommandError> {
        let envelope = self.envelope(url).await?;
        let code = envelope
            .get("code")
            .and_then(Value::as_i64)
            .ok_or_else(|| {
                CommandError::new(
                    "VIDEO_API_INVALID",
                    format!("{}：接口缺少状态码", endpoint(url)),
                )
            })?;
        if code != 0 {
            return Err(api_error(code, url, self.cookie.is_logged_in()));
        }
        Ok(envelope.get("data").cloned().unwrap_or(Value::Null))
    }

    /// 请求接口并返回完整信封，供登录态校验等需要读取 code 的场景使用。
    pub(crate) async fn envelope(&self, url: &str) -> Result<Value, CommandError> {
        let response = self.with_retry(url, REFERER, true).await?;
        response.json::<Value>().await.map_err(|error| {
            eprintln!(
                "VIDEO_API_PARSE({}): {}",
                endpoint(url),
                error.without_url()
            );
            CommandError::new("VIDEO_API_INVALID", "B 站接口返回内容无法解析")
        })
    }

    /// 登录接口必须保留响应头 Cookie；只读取 JSON 会丢失成功登录凭据。
    pub(crate) async fn login_response(
        &self,
        url: &str,
    ) -> Result<(Value, reqwest::header::HeaderMap), CommandError> {
        let response = self.with_retry(url, REFERER, true).await?;
        let headers = response.headers().clone();
        let envelope = response
            .json::<Value>()
            .await
            .map_err(|_| CommandError::new("VIDEO_API_INVALID", "B 站登录接口返回内容无法解析"))?;
        let code = envelope
            .get("code")
            .and_then(Value::as_i64)
            .ok_or_else(|| CommandError::new("VIDEO_API_INVALID", "B 站登录接口缺少状态码"))?;
        if code != 0 {
            return Err(api_error(code, url, self.cookie.is_logged_in()));
        }
        Ok((
            envelope.get("data").cloned().unwrap_or(Value::Null),
            headers,
        ))
    }

    /// 下载文本资源（字幕等），必须携带来源与跨域头。
    pub(crate) async fn text(&self, url: &str, referer: &str) -> Result<String, CommandError> {
        let response = self.with_retry(url, referer, true).await?;
        response.text().await.map_err(|error| {
            eprintln!("VIDEO_TEXT({}): {}", endpoint(url), error.without_url());
            CommandError::new("VIDEO_DOWNLOAD_FAILED", "下载 B 站内容失败")
        })
    }

    /// 发起一次可流式读取的请求；下载循环自行重试与取消。
    pub(crate) async fn stream(&self, url: &str, referer: &str) -> Result<Response, CommandError> {
        let response = self.send(url, referer, true).await?;
        Ok(response)
    }

    /// 本地媒体代理只转发单段字节范围，其余头由适配器控制。
    pub(crate) async fn stream_range(
        &self,
        url: &str,
        referer: &str,
        range: Option<&str>,
    ) -> Result<Response, CommandError> {
        if let Some(range) = range {
            let valid = range
                .strip_prefix("bytes=")
                .and_then(|value| value.split_once('-'))
                .is_some_and(|(start, end)| {
                    (!start.is_empty() || !end.is_empty())
                        && start.bytes().all(|b| b.is_ascii_digit())
                        && end.bytes().all(|b| b.is_ascii_digit())
                });
            if !valid || range.len() > 64 {
                return Err(super::targets::invalid());
            }
        }
        self.send_range(url, referer, true, range).await
    }

    /// 仅对网络故障退避重试，服务端拒绝时立即返回准确状态。
    async fn with_retry(
        &self,
        url: &str,
        referer: &str,
        origin: bool,
    ) -> Result<Response, CommandError> {
        let mut last = None;
        for (attempt, delay) in std::iter::once(0u64).chain(RETRY_DELAYS).enumerate() {
            if delay > 0 {
                tokio::time::sleep(Duration::from_secs(delay)).await;
            }
            match self.send(url, referer, origin).await {
                Ok(response) => return Ok(response),
                Err(error) if error.code == "VIDEO_NETWORK_ERROR" => {
                    eprintln!("VIDEO_RETRY(attempt={attempt}): {error}");
                    last = Some(error);
                }
                Err(error) => return Err(error),
            }
        }
        Err(last.unwrap_or_else(|| CommandError::new("VIDEO_NETWORK_ERROR", "请求 B 站失败")))
    }

    /// 每跳校验地址并重新构造请求，禁止自动转发 Cookie 或泄漏签名地址。
    async fn send(&self, url: &str, referer: &str, origin: bool) -> Result<Response, CommandError> {
        self.send_range(url, referer, origin, None).await
    }

    /// Range 请求与普通请求共用逐跳校验，206 仅在范围读取时视为成功。
    async fn send_range(
        &self,
        url: &str,
        referer: &str,
        origin: bool,
        range: Option<&str>,
    ) -> Result<Response, CommandError> {
        let mut target = super::targets::validate(url)?;
        let page_only = super::targets::page(&target);
        let auth_only = super::targets::credentials(&target) && !page_only;
        for _ in 0..6 {
            let mut request = self.request(&target, referer, origin)?;
            if let Some(range) = range {
                request = request.header(reqwest::header::RANGE, range);
            }
            let response = request.send().await.map_err(|_| {
                CommandError::new("VIDEO_NETWORK_ERROR", "网络请求失败，请检查网络后重试")
            })?;
            if super::targets::credentials(&target) {
                self.received
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .absorb(response.headers());
            }
            if response.status().is_redirection() {
                let location = response
                    .headers()
                    .get(reqwest::header::LOCATION)
                    .and_then(|value| value.to_str().ok())
                    .ok_or_else(super::targets::invalid)?;
                if location.chars().any(char::is_control) || location.contains('\\') {
                    return Err(super::targets::invalid());
                }
                let next = target
                    .join(location)
                    .map_err(|_| super::targets::invalid())?;
                target = super::targets::validate(next.as_str())?;
                if (page_only && !super::targets::page(&target))
                    || (auth_only && !super::targets::credentials(&target))
                {
                    return Err(super::targets::invalid());
                }
                continue;
            }
            if response.status().as_u16() != 200
                && !(range.is_some() && response.status().as_u16() == 206)
            {
                return Err(http_error(
                    response.status().as_u16(),
                    target.as_str(),
                    super::targets::credentials(&target) && self.session_cookie().is_logged_in(),
                ));
            }
            return Ok(response);
        }
        Err(CommandError::new(
            "VIDEO_REDIRECT_LIMIT",
            "B 站资源跳转次数过多",
        ))
    }

    /// 请求头使用固定安全来源；会话头标记敏感以避免调试输出凭据。
    fn request(
        &self,
        target: &reqwest::Url,
        referer: &str,
        origin: bool,
    ) -> Result<reqwest::RequestBuilder, CommandError> {
        let safe_referer = super::targets::validate(referer)
            .ok()
            .filter(super::targets::page)
            .map(|mut url| {
                url.set_query(None);
                url.set_fragment(None);
                url.to_string()
            })
            .unwrap_or_else(|| REFERER.to_string());
        let mut request = self
            .http
            .get(target.clone())
            .header("User-Agent", USER_AGENT)
            .header("Referer", safe_referer)
            .header("Accept-Language", "zh-CN,zh;q=0.9");
        if origin {
            request = request.header("Origin", ORIGIN);
        }
        if super::targets::credentials(target) {
            if let Some(cookie) = self.session_cookie().header() {
                let cookie = Zeroizing::new(cookie);
                let mut header =
                    reqwest::header::HeaderValue::from_str(cookie.as_str()).map_err(|_| {
                        CommandError::new("VIDEO_LOGIN_INVALID", "登录凭据格式无效，请重新登录")
                    })?;
                header.set_sensitive(true);
                request = request.header(reqwest::header::COOKIE, header);
            }
        }
        Ok(request)
    }

    /// 登录申请与轮询共用会话，保留中途下发的设备 Cookie 并在确认时落库。
    pub(crate) fn session_cookie(&self) -> CookieJar {
        let mut cookie = self.cookie.clone();
        cookie.merge(&self.received.lock().unwrap_or_else(|e| e.into_inner()));
        cookie
    }
}
