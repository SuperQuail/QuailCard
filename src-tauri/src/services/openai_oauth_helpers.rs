use std::{
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    time::Duration,
};

use base64::{engine::general_purpose, Engine as _};
use reqwest::Url;
use serde::Deserialize;
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpStream,
    sync::Notify,
};
use uuid::Uuid;

use crate::{
    database::{now_timestamp, Database},
    error::CommandError,
    models::OpenAiOAuthCredential,
    vault::EncryptedVault,
};

pub(super) const CLIENT_ID: &str = "app_EMoamEEZ73f0CkXaXp7hrann";
pub(super) const ISSUER: &str = "https://auth.openai.com";
pub(super) const CALLBACK_URI: &str = "http://localhost:1455/auth/callback";
pub(super) const DEVICE_REDIRECT_URI: &str = "https://auth.openai.com/deviceauth/callback";
pub(super) const CALLBACK_ADDRESS: &str = "127.0.0.1:1455";
pub(super) const USER_AGENT: &str = "QuailCard/0.1.0";

/// OpenAI OAuth 令牌端点响应。
#[derive(Deserialize)]
pub(super) struct TokenResponse {
    pub(super) id_token: Option<String>,
    pub(super) access_token: String,
    pub(super) refresh_token: Option<String>,
    pub(super) expires_in: Option<i64>,
}

/// OpenAI 设备码初始化响应。
#[derive(Deserialize)]
pub(super) struct DeviceCodeResponse {
    pub(super) device_auth_id: String,
    pub(super) user_code: String,
    pub(super) interval: serde_json::Value,
}

/// OpenAI 设备码轮询成功响应。
#[derive(Deserialize)]
pub(super) struct DeviceTokenResponse {
    pub(super) authorization_code: String,
    pub(super) code_verifier: String,
}

/// JWT 中用于路由 ChatGPT 请求的非敏感声明。
#[derive(Deserialize)]
struct TokenClaims {
    chatgpt_account_id: Option<String>,
    #[serde(rename = "https://api.openai.com/auth")]
    openai_auth: Option<OpenAiAuthClaims>,
    organizations: Option<Vec<OrganizationClaim>>,
}

/// JWT 命名空间中的 OpenAI 账号声明。
#[derive(Deserialize)]
struct OpenAiAuthClaims {
    chatgpt_account_id: Option<String>,
}

/// JWT 中的组织摘要。
#[derive(Deserialize)]
struct OrganizationClaim {
    id: String,
}

/// 从 HTTP 请求首行提取并校验 OAuth 回调参数。
pub(super) fn parse_callback_request(
    request: &[u8],
    expected_state: &str,
) -> Result<String, CommandError> {
    let text = String::from_utf8_lossy(request);
    let target = text
        .lines()
        .next()
        .and_then(|line| line.split_whitespace().nth(1))
        .ok_or_else(|| CommandError::new("OAUTH_CALLBACK_ERROR", "授权回调格式无效"))?;
    let url = Url::parse(&format!("http://localhost{target}"))
        .map_err(|_| CommandError::new("OAUTH_CALLBACK_ERROR", "授权回调地址无效"))?;
    if url.path() != "/auth/callback" {
        return Err(CommandError::new(
            "OAUTH_CALLBACK_ERROR",
            "授权回调路径无效",
        ));
    }
    let state = url
        .query_pairs()
        .find(|(key, _)| key == "state")
        .map(|(_, value)| value.into_owned());
    if state.as_deref() != Some(expected_state) {
        return Err(CommandError::new(
            "OAUTH_STATE_MISMATCH",
            "OAuth state 校验失败",
        ));
    }
    if let Some(error) = url
        .query_pairs()
        .find(|(key, _)| key == "error_description" || key == "error")
        .map(|(_, value)| value.into_owned())
    {
        return Err(CommandError::new(
            "OAUTH_AUTHORIZATION_REJECTED",
            format!("OpenAI 授权失败：{error}"),
        ));
    }
    url.query_pairs()
        .find(|(key, _)| key == "code")
        .map(|(_, value)| value.into_owned())
        .filter(|value| !value.is_empty())
        .ok_or_else(|| CommandError::new("OAUTH_CALLBACK_ERROR", "授权回调缺少授权码"))
}

/// 构造限制为 OpenAI 固定回调地址的 PKCE 授权 URL。
pub(super) fn authorize_url(challenge: &str, state: &str) -> Result<String, CommandError> {
    let mut url = Url::parse(&format!("{ISSUER}/oauth/authorize"))
        .map_err(|_| CommandError::new("OAUTH_URL_ERROR", "无法构造 OpenAI 授权地址"))?;
    url.query_pairs_mut()
        .append_pair("response_type", "code")
        .append_pair("client_id", CLIENT_ID)
        .append_pair("redirect_uri", CALLBACK_URI)
        .append_pair("scope", "openid profile email offline_access")
        .append_pair("code_challenge", challenge)
        .append_pair("code_challenge_method", "S256")
        .append_pair("id_token_add_organizations", "true")
        .append_pair("codex_cli_simplified_flow", "true")
        .append_pair("state", state)
        .append_pair("originator", "quailcard");
    Ok(url.to_string())
}

/// 使用 UUID v7 的随机位生成符合 PKCE 字符集的高熵文本。
pub(super) fn generate_random_text(parts: usize) -> String {
    (0..parts)
        .map(|_| Uuid::now_v7().simple().to_string())
        .collect()
}

/// 将 OpenAI token 响应转换为保险库持久化格式。
pub(super) fn credential_from_tokens(
    tokens: TokenResponse,
    fallback_refresh_token: Option<&str>,
) -> Result<OpenAiOAuthCredential, CommandError> {
    let account_id = extract_account_id(tokens.id_token.as_deref(), &tokens.access_token);
    let refresh_token = tokens
        .refresh_token
        .or_else(|| fallback_refresh_token.map(str::to_string))
        .ok_or_else(|| CommandError::new("OAUTH_RESPONSE_ERROR", "OAuth 响应缺少 refresh token"))?;
    Ok(OpenAiOAuthCredential {
        access_token: tokens.access_token,
        refresh_token,
        expires_at: now_timestamp() + tokens.expires_in.unwrap_or(3600).max(60),
        account_id,
    })
}

/// 解析令牌端点响应，避免泄露响应正文。
pub(super) async fn parse_token_response(
    response: reqwest::Response,
    rejection_message: &str,
) -> Result<TokenResponse, CommandError> {
    if !response.status().is_success() {
        return Err(CommandError::new(
            "OAUTH_TOKEN_REJECTED",
            format!("{rejection_message}（HTTP {}）", response.status().as_u16()),
        ));
    }
    response
        .json::<TokenResponse>()
        .await
        .map_err(|_| CommandError::new("OAUTH_RESPONSE_ERROR", "OAuth token 响应格式无效"))
}

/// 从 JWT payload 提取 ChatGPT 账号 ID；令牌真实性由 TLS token 端点保证。
fn extract_account_id(id_token: Option<&str>, access_token: &str) -> Option<String> {
    id_token
        .and_then(extract_account_claim)
        .or_else(|| extract_account_claim(access_token))
}

/// 解码单个 JWT payload 中的账号路由声明。
pub(super) fn extract_account_claim(token: &str) -> Option<String> {
    let payload = token.split('.').nth(1)?;
    let decoded = general_purpose::URL_SAFE_NO_PAD
        .decode(payload)
        .or_else(|_| general_purpose::URL_SAFE.decode(payload))
        .ok()?;
    let claims = serde_json::from_slice::<TokenClaims>(&decoded).ok()?;
    claims
        .chatgpt_account_id
        .or_else(|| claims.openai_auth.and_then(|auth| auth.chatgpt_account_id))
        .or_else(|| {
            claims
                .organizations
                .and_then(|organizations| organizations.into_iter().next())
                .map(|organization| organization.id)
        })
}

/// 将设备码返回的字符串或数字轮询间隔限制在安全范围内。
pub(super) fn parse_poll_interval(value: &serde_json::Value) -> Duration {
    let seconds = value
        .as_u64()
        .or_else(|| value.as_str().and_then(|text| text.parse::<u64>().ok()))
        .unwrap_or(5)
        .max(1);
    Duration::from_secs(seconds)
}

/// 在固定上限内读取完整 HTTP 请求头，兼容 TCP 分片。
pub(super) async fn read_callback_headers(stream: &mut TcpStream) -> Result<Vec<u8>, CommandError> {
    let mut request = Vec::with_capacity(1024);
    let mut chunk = [0_u8; 1024];
    while request.len() < 8192 {
        let length = stream
            .read(&mut chunk)
            .await
            .map_err(|_| CommandError::new("OAUTH_CALLBACK_ERROR", "无法读取浏览器授权回调"))?;
        if length == 0 {
            break;
        }
        request.extend_from_slice(&chunk[..length]);
        if request.windows(4).any(|window| window == b"\r\n\r\n") {
            return Ok(request);
        }
    }
    Err(CommandError::new(
        "OAUTH_CALLBACK_ERROR",
        "授权回调请求头不完整或超过限制",
    ))
}

/// 向系统浏览器返回不包含动态 HTML 的 OAuth 结果页。
pub(super) async fn write_callback_page(
    stream: &mut TcpStream,
    status: &str,
    title: &str,
    detail: &str,
) {
    let body = format!(
        "<!doctype html><meta charset=\"utf-8\"><title>{title}</title><body style=\"font-family:system-ui;padding:40px;background:#f4f1e8;color:#24342b\"><h1>{title}</h1><p>{detail}</p></body>"
    );
    let response = format!(
        "HTTP/1.1 {status}\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );
    let _ = stream.write_all(response.as_bytes()).await;
    let _ = stream.shutdown().await;
}

/// 提供给 Codex Responses 请求的短期访问凭据。
pub(crate) struct OpenAiAccess {
    pub access_token: String,
    pub account_id: Option<String>,
}

/// 为后台登录任务提供可唤醒的取消信号。
#[derive(Clone)]
pub(super) struct LoginCancellation {
    pub(super) cancelled: Arc<AtomicBool>,
    pub(super) notify: Arc<Notify>,
}

impl LoginCancellation {
    /// 创建尚未取消的登录信号。
    pub(super) fn new() -> Self {
        Self {
            cancelled: Arc::new(AtomicBool::new(false)),
            notify: Arc::new(Notify::new()),
        }
    }

    /// 标记任务取消并唤醒等待中的网络请求。
    pub(super) fn cancel(&self) {
        self.cancelled.store(true, Ordering::Release);
        self.notify.notify_waiters();
    }

    /// 查询登录是否已经取消。
    pub(super) fn is_cancelled(&self) -> bool {
        self.cancelled.load(Ordering::Acquire)
    }

    /// 异步等待取消信号，避免在检查与等待之间丢失通知。
    pub(super) async fn cancelled(&self) {
        let notified = self.notify.notified();
        if self.is_cancelled() {
            return;
        }
        notified.await;
    }
}

/// 创建不会暴露内部任务状态的登录取消错误。
pub(super) fn cancelled_error() -> CommandError {
    CommandError::new("OAUTH_CANCELLED", "ChatGPT 登录已取消")
}

/// 将持久化凭据裁剪为模型请求所需字段。
pub(super) fn to_access(credential: OpenAiOAuthCredential) -> OpenAiAccess {
    OpenAiAccess {
        access_token: credential.access_token,
        account_id: credential.account_id,
    }
}

/// 从加密保险库读取并校验 OAuth JSON。
pub(super) async fn read_oauth_credential(
    vault: &EncryptedVault,
    database: &Database,
    secret_ref: &str,
) -> Result<OpenAiOAuthCredential, CommandError> {
    let serialized = vault.get_credential(database, secret_ref).await?;
    serde_json::from_str(&serialized)
        .map_err(|_| CommandError::new("OAUTH_CREDENTIAL_ERROR", "加密保险库中的 OAuth 数据无效"))
}
