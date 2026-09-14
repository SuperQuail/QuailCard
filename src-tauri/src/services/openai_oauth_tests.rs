use super::super::openai_oauth_helpers::{extract_account_claim, parse_callback_request};
use base64::{engine::general_purpose, Engine as _};

#[test]
/// 授权回调必须同时包含匹配的 state 和授权码。
fn validates_browser_callback() {
    let request =
        b"GET /auth/callback?code=code-1&state=state-1 HTTP/1.1\r\nHost: localhost\r\n\r\n";
    assert_eq!(
        parse_callback_request(request, "state-1").expect("应解析授权码"),
        "code-1"
    );
    assert!(parse_callback_request(request, "other-state").is_err());
}

#[test]
/// JWT 账号提取兼容 OpenAI 命名空间声明。
fn extracts_namespaced_account_claim() {
    let payload = general_purpose::URL_SAFE_NO_PAD
        .encode(br#"{"https://api.openai.com/auth":{"chatgpt_account_id":"account-1"}}"#);
    let token = format!("header.{payload}.signature");
    assert_eq!(extract_account_claim(&token).as_deref(), Some("account-1"));
}
