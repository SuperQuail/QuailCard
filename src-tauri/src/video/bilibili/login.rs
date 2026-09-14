//! B 站扫码登录：二维码生成、轮询状态机与登录态校验。

use base64::{engine::general_purpose::STANDARD, Engine as _};
use serde_json::Value;

use super::http::{BiliClient, CookieJar};
use crate::error::CommandError;

/// 二维码生成接口；source 与会话级风控相关。
const GENERATE_URL: &str =
    "https://passport.bilibili.com/x/passport-login/web/qrcode/generate?source=main-fe-header";
/// 轮询接口，每秒一次。
const POLL_PREFIX: &str =
    "https://passport.bilibili.com/x/passport-login/web/qrcode/poll?qrcode_key=";

/// 扫码页允许的官方主机：B 站已把二维码内容从 passport 迁到 account 子域。
const QR_HOSTS: [&str; 4] = [
    "passport.bilibili.com",
    "account.bilibili.com",
    "www.bilibili.com",
    "m.bilibili.com",
];

/// 一次登录会话的二维码信息。
#[derive(Clone, PartialEq)]
pub(crate) struct QrStart {
    /// 二维码内容（B 站登录页地址）。
    pub url: String,
    /// 轮询键。
    pub key: String,
    /// 可直接放进 img 标签的 SVG 数据地址。
    pub image: String,
}

impl Drop for QrStart {
    /// 二维码与轮询键同为短期凭据，会话结束时清理副本。
    fn drop(&mut self) {
        use zeroize::Zeroize;
        self.url.zeroize();
        self.key.zeroize();
        self.image.zeroize();
    }
}

impl std::fmt::Debug for QrStart {
    /// 二维码调试输出不能泄漏扫描地址或会话键。
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("QrStart").finish_non_exhaustive()
    }
}

/// 轮询结果。
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum LoginState {
    /// 等待扫码。
    Waiting,
    /// 已扫码等待确认。
    Scanned,
    /// 已确认，携带 Cookie。
    Confirmed(Box<CookieJar>),
    /// 二维码过期。
    Expired,
}

/// 申请二维码并渲染 SVG，前端用 img 直接展示。
pub(crate) async fn start(client: &BiliClient) -> Result<QrStart, CommandError> {
    let data = client.api(GENERATE_URL).await?;
    let url = data
        .get("url")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_string();
    let key = data
        .get("qrcode_key")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_string();
    validate_qr(&url, &key)?;
    let image = render_qr_svg(&url)?;
    Ok(QrStart { url, key, image })
}

/// 校验二维码地址与轮询键；失败只给可操作提示，不回显服务端内容。
fn validate_qr(url: &str, key: &str) -> Result<(), CommandError> {
    let official = super::targets::validate(url).ok().is_some_and(|parsed| {
        parsed
            .host_str()
            .is_some_and(|host| QR_HOSTS.contains(&host))
    });
    if official && safe_key(key) {
        return Ok(());
    }
    Err(CommandError::new(
        "VIDEO_LOGIN_FAILED",
        "无法获取登录二维码，请稍后重试",
    ))
}

/// 轮询键只允许 URL 安全字符，避免把外部内容拼进后续请求地址。
fn safe_key(key: &str) -> bool {
    !key.is_empty()
        && key.len() <= 256
        && key
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
}

/// 轮询一次登录状态。
pub(crate) async fn poll(client: &BiliClient, key: &str) -> Result<LoginState, CommandError> {
    if !safe_key(key) {
        return Err(CommandError::new(
            "VIDEO_LOGIN_INVALID",
            "二维码会话无效，请重新获取二维码",
        ));
    }
    let url = zeroize::Zeroizing::new(format!("{POLL_PREFIX}{key}&source=main-fe-header"));
    let (data, headers) = client.login_response(&url).await?;
    let state = interpret(&data, &headers)?;
    if let LoginState::Confirmed(jar) = state {
        let mut merged = client.session_cookie();
        merged.merge(&jar);
        Ok(LoginState::Confirmed(Box::new(merged)))
    } else {
        Ok(state)
    }
}

/// 状态码含义：0 成功、86090 已扫码、86101 未扫码、86038 过期。
fn interpret(
    data: &Value,
    headers: &reqwest::header::HeaderMap,
) -> Result<LoginState, CommandError> {
    let code = data.get("code").and_then(Value::as_i64).unwrap_or(-1);
    match code {
        0 => {
            let callback = data.get("url").and_then(Value::as_str).unwrap_or("");
            let jar = login_cookies(callback, headers);
            if jar.is_logged_in() {
                Ok(LoginState::Confirmed(Box::new(jar)))
            } else {
                Err(CommandError::new(
                    "VIDEO_LOGIN_CREDENTIAL_MISSING",
                    "B 站已确认登录，但未收到登录凭据，请重新获取二维码后重试",
                ))
            }
        }
        86090 => Ok(LoginState::Scanned),
        86101 => Ok(LoginState::Waiting),
        86038 => Ok(LoginState::Expired),
        _ => Err(CommandError::new(
            "VIDEO_API_INVALID",
            "B 站登录接口返回未知状态",
        )),
    }
}

/// 兼容回调 URL 与 Set-Cookie 两种凭据来源；响应头优先且不混入 Cookie 属性。
fn login_cookies(callback: &str, headers: &reqwest::header::HeaderMap) -> CookieJar {
    let mut jar = CookieJar::from_login_url(callback);
    jar.absorb(headers);
    jar
}

/// 生成二维码 SVG 数据地址；渲染失败按登录失败处理。
pub(crate) fn render_qr_svg(content: &str) -> Result<String, CommandError> {
    let code = qrcode::QrCode::new(content.as_bytes())
        .map_err(|_| CommandError::new("VIDEO_LOGIN_FAILED", "二维码内容无效"))?;
    let svg = code
        .render::<qrcode::render::svg::Color>()
        .min_dimensions(220, 220)
        .quiet_zone(true)
        .build();
    Ok(format!(
        "data:image/svg+xml;base64,{}",
        STANDARD.encode(svg)
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use reqwest::header::{HeaderMap, HeaderValue, SET_COOKIE};
    use serde_json::json;

    #[test]
    /// 只有服务端明确的过期码才显示过期，未知响应应报错。
    fn maps_poll_codes() {
        let headers = HeaderMap::new();
        for (code, state) in [
            (86101, LoginState::Waiting),
            (86090, LoginState::Scanned),
            (86038, LoginState::Expired),
        ] {
            assert_eq!(interpret(&json!({"code": code}), &headers).unwrap(), state);
        }
        assert!(interpret(&json!({"code": 999}), &headers).is_err());
        assert!(interpret(&json!({}), &headers).is_err());
    }

    #[test]
    /// 实测扫码页已迁移到 account 子域；只接受官方 HTTPS 页面与 URL 安全轮询键。
    fn validates_migrated_qr_page() {
        let observed = "https://account.bilibili.com/h5/account-h5/auth/scan-web?navhide=1&callback=close&qrcode_key=6cea02f216986912c79084a111f0c341&from=main-fe-header";
        assert!(validate_qr(observed, "6cea02f216986912c79084a111f0c341").is_ok());
        assert!(validate_qr(
            "https://passport.bilibili.com/h5/login?qrcode_key=a",
            "a-b_1"
        )
        .is_ok());
        let too_long = "k".repeat(257);
        for (url, key) in [
            ("https://account.bilibili.com.evil.test/h5?x=1", "ok"),
            ("http://account.bilibili.com/h5?x=1", "ok"),
            ("https://evil.test/h5?x=1", "ok"),
            (observed, ""),
            (observed, "bad key"),
            (observed, "bad&query=1"),
            (observed, too_long.as_str()),
        ] {
            let error = validate_qr(url, key).unwrap_err();
            assert_eq!(error.code, "VIDEO_LOGIN_FAILED");
            assert!(!error.message.contains("account.bilibili.com"));
        }
    }

    #[test]
    /// 保留 BBDown 使用的回调 URL 凭据兼容路径。
    fn confirms_from_callback() {
        let state = interpret(
            &json!({"code": 0, "url": "https://passport.bilibili.com/?SESSDATA=s%2Ct&bili_jct=j"}),
            &HeaderMap::new(),
        )
        .unwrap();
        let LoginState::Confirmed(jar) = state else {
            panic!("应确认登录")
        };
        assert_eq!(jar.sessdata, "s%2Ct");
        assert_eq!(jar.bili_jct, "j");
    }

    #[test]
    /// 成功但缺少凭据是接收失败，绝不能误报二维码过期。
    fn missing_credentials_are_not_expired() {
        let error = interpret(
            &json!({"code": 0, "url": "https://passport.bilibili.com/?bili_jct=j"}),
            &HeaderMap::new(),
        )
        .unwrap_err();
        assert_eq!(error.code, "VIDEO_LOGIN_CREDENTIAL_MISSING");
        assert!(!error.message.contains("过期"));
    }

    #[test]
    /// 多个 Set-Cookie 分别读取，保留编码和等号，不保存 Path 等属性。
    fn confirms_from_response_headers() {
        let mut headers = HeaderMap::new();
        headers.append(
            SET_COOKIE,
            HeaderValue::from_static("SESSDATA=new%2Ctoken==; Path=/; HttpOnly; Secure"),
        );
        headers.append(
            SET_COOKIE,
            HeaderValue::from_static(
                "bili_jct=csrf; Expires=Wed, 21 Oct 2030 07:28:00 GMT; Path=/",
            ),
        );
        let state = interpret(
            &json!({"code": 0, "url": "https://passport.bilibili.com/?DedeUserID=9&SESSDATA=old"}),
            &headers,
        )
        .unwrap();
        let LoginState::Confirmed(jar) = state else {
            panic!("应确认登录")
        };
        assert_eq!(jar.sessdata, "new%2Ctoken==");
        assert_eq!(jar.bili_jct, "csrf");
        assert_eq!(jar.dede_user_id, "9");
        assert!(!jar.encode().contains("Path"));
        assert_eq!(
            interpret(&json!({"code": 86038}), &headers).unwrap(),
            LoginState::Expired
        );
    }

    #[tokio::test]
    #[ignore = "需要真实访问 B 站登录接口"]
    /// 用应用自己的客户端走一遍二维码申请，确认迁移后的扫码页仍能渲染。
    async fn live_qr_start_renders_migrated_page() {
        let client = BiliClient::new(CookieJar::default()).unwrap();
        let started = start(&client).await.expect("真实二维码申请");
        assert!(started.image.starts_with("data:image/svg+xml;base64,"));
        assert!(!started.key.is_empty() && started.key.len() <= 256);
        assert!(super::super::targets::validate(&started.url).is_ok());
    }

    #[tokio::test]
    /// 登录响应不能从本地或外部地址读取，避免凭据来源混淆。
    async fn rejects_untrusted_login_response() {
        let client = BiliClient::new(CookieJar::default()).unwrap();
        assert_eq!(
            client
                .login_response("http://127.0.0.1/poll")
                .await
                .unwrap_err()
                .code,
            "VIDEO_URL_INVALID"
        );
    }

    #[test]
    /// 二维码渲染为 SVG 数据地址。
    fn renders_svg_data_url() {
        let image = render_qr_svg("https://passport.bilibili.com/h5/").unwrap();
        assert!(image.starts_with("data:image/svg+xml;base64,"));
    }
}
