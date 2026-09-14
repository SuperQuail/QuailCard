//! OpenAI OAuth 令牌端点协议层。
//!
//! 本模块只负责与 OpenAI token 端点（`/oauth/token`）的具体协议交互：
//! 授权码换令牌（PKCE）与 refresh token 换新访问令牌。它只依赖一个已配置的
//! HTTP 客户端，不接触 SQLite、Tauri 或会话状态，保证协议细节与会话编排解耦。

use reqwest::Client;

use super::openai_oauth_helpers::{parse_token_response, TokenResponse, CLIENT_ID, ISSUER};
use crate::error::CommandError;

/// 使用授权码和 PKCE verifier 向 token 端点交换 OAuth 令牌。
///
/// 授权码来自浏览器回调或设备码轮询；redirect_uri 与 code_verifier 必须与
/// 发起授权时一致，否则端点会拒绝。失败信息只保留安全的 HTTP 状态码，
/// 不把令牌端点响应正文或任何敏感字段写入错误消息。
pub(super) async fn exchange_code(
    client: &Client,
    code: &str,
    redirect_uri: &str,
    verifier: &str,
) -> Result<TokenResponse, CommandError> {
    let response = client
        .post(format!("{ISSUER}/oauth/token"))
        .form(&[
            ("grant_type", "authorization_code"),
            ("code", code),
            ("redirect_uri", redirect_uri),
            ("client_id", CLIENT_ID),
            ("code_verifier", verifier),
        ])
        .send()
        .await
        .map_err(|_| CommandError::new("OAUTH_NETWORK_ERROR", "无法交换 OpenAI OAuth 令牌"))?;
    parse_token_response(response, "授权码已失效或被拒绝").await
}

/// 使用长期 refresh token 换取新的短期访问令牌。
///
/// refresh token 属于长期敏感凭据，仅作为请求表单字段发送，绝不进入日志或
/// 错误消息。端点拒绝时只回传安全的拒绝提示，不泄露令牌正文。
pub(super) async fn refresh_tokens(
    client: &Client,
    refresh_token: &str,
) -> Result<TokenResponse, CommandError> {
    let response = client
        .post(format!("{ISSUER}/oauth/token"))
        .form(&[
            ("grant_type", "refresh_token"),
            ("refresh_token", refresh_token),
            ("client_id", CLIENT_ID),
        ])
        .send()
        .await
        .map_err(|_| CommandError::new("OAUTH_NETWORK_ERROR", "无法刷新 OpenAI OAuth 令牌"))?;
    parse_token_response(response, "ChatGPT 登录已失效，请重新登录").await
}
