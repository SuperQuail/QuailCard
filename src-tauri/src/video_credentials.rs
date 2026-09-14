//! 视频凭据读取与登录校验策略；不把保险库故障或网络故障降级成游客。

use serde_json::Value;
use zeroize::Zeroizing;

use crate::{
    error::CommandError,
    video::bilibili::http::{api_error, BiliClient, CookieJar},
};

const NAV_URL: &str = "https://api.bilibili.com/x/web-interface/nav";

/// 仅确切的凭据缺失允许游客模式；原始秘密在所有退出路径自动清零。
pub(crate) fn load(result: Result<String, CommandError>) -> Result<CookieJar, CommandError> {
    let raw = match result {
        Ok(raw) => Zeroizing::new(raw),
        Err(error) if error.code == "PROVIDER_CREDENTIAL_MISSING" => {
            return Ok(CookieJar::default());
        }
        Err(error) => return Err(error),
    };
    if raw.is_empty() {
        return Ok(CookieJar::default());
    }
    // 不允许解码器静默忽略坏片段或请求头控制字符。
    if raw.bytes().any(|byte| byte.is_ascii_control())
        || raw
            .split(';')
            .filter(|part| !part.trim().is_empty())
            .any(|part| {
                part.split_once('=')
                    .is_none_or(|(key, _)| key.trim().is_empty())
            })
    {
        return Err(invalid_credential());
    }
    let jar = CookieJar::decode(&raw);
    if !jar.is_logged_in() {
        return Err(invalid_credential());
    }
    Ok(jar)
}

/// 登录档案：nav 返回的昵称与头像，缺失时为空串。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct LoginProfile {
    pub name: String,
    pub avatar: String,
}

/// 游客不发起网络请求；非空凭据必须经过主站的实时登录校验。
/// 返回 None 表示确认处于未登录状态，而不是网络或凭据故障。
pub(crate) async fn validate(client: &BiliClient) -> Result<Option<LoginProfile>, CommandError> {
    if !client.cookie().is_logged_in() {
        return Ok(None);
    }
    Ok(Some(validate_response(client.envelope(NAV_URL).await)?))
}

/// 区分凭据失效、接口业务拒绝、畸形响应与传输错误，禁止猜测缺失字段。
fn validate_response(response: Result<Value, CommandError>) -> Result<LoginProfile, CommandError> {
    let envelope = response?;
    let code = envelope
        .get("code")
        .and_then(Value::as_i64)
        .ok_or_else(invalid_response)?;
    if code != 0 {
        return Err(api_error(code, NAV_URL, true));
    }
    let data = envelope.get("data").ok_or_else(invalid_response)?;
    match data.get("isLogin").and_then(Value::as_bool) {
        Some(true) => Ok(profile(data)),
        Some(false) => Err(api_error(-101, NAV_URL, true)),
        None => Err(invalid_response()),
    }
}

/// 昵称与头像只当普通文本使用；限制长度避免异常响应撑大前端状态。
fn profile(data: &Value) -> LoginProfile {
    let field = |key: &str, limit: usize| {
        data.get(key)
            .and_then(Value::as_str)
            .unwrap_or_default()
            .trim()
            .chars()
            .take(limit)
            .collect::<String>()
    };
    LoginProfile {
        name: field("uname", 64),
        avatar: field("face", 512),
    }
}

/// 损坏凭据只给出安全提示，不回显任何原始 Cookie。
pub(crate) fn invalid_credential() -> CommandError {
    CommandError::new(
        "VIDEO_CREDENTIAL_INVALID",
        "B 站登录凭据无效，请重新扫码登录",
    )
}

/// 接口结构不完整属于协议故障，而不是已确认的登录失效。
fn invalid_response() -> CommandError {
    CommandError::new("VIDEO_API_INVALID", "B 站登录校验接口返回内容无效")
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    /// 只有约定的缺失码和空值允许游客，不能吞掉保险库状态错误。
    #[test]
    fn missing_is_distinct_from_locked_and_corrupt() {
        assert!(!load(Err(CommandError::new(
            "PROVIDER_CREDENTIAL_MISSING",
            "缺失"
        )))
        .unwrap()
        .is_logged_in());
        assert!(!load(Ok(String::new())).unwrap().is_logged_in());
        for code in [
            "VAULT_LOCKED",
            "VAULT_DATA_INVALID",
            "VAULT_CRYPTO_ERROR",
            "FILE_ERROR",
            "PROVIDER_CREDENTIAL_MISSING_EXTRA",
        ] {
            let error = load(Err(CommandError::new(code, "安全错误"))).unwrap_err();
            assert_eq!(error.code, code);
        }
    }

    /// 非空但无法用于登录的凭据不得默认为游客或进入请求头。
    #[test]
    fn rejects_nonempty_invalid_credentials() {
        for raw in [
            " ",
            "garbage",
            "bili_jct=token",
            "SESSDATA=",
            "SESSDATA=token; broken",
            "SESSDATA=token\r\nX-Foo: bar",
        ] {
            assert_eq!(
                load(Ok(raw.to_string())).unwrap_err().code,
                "VIDEO_CREDENTIAL_INVALID"
            );
        }
        assert!(load(Ok("SESSDATA=token; bili_jct=csrf".to_string()))
            .unwrap()
            .is_logged_in());
    }

    /// 登录成功必须同时具有数值零状态码和布尔真标志。
    #[test]
    fn requires_explicit_login_success() {
        assert!(validate_response(Ok(json!({"code": 0, "data": {"isLogin": true}}))).is_ok());
        for body in [
            json!({}),
            json!({"data": {"isLogin": true}}),
            json!({"code": "0", "data": {"isLogin": true}}),
            json!({"code": 0}),
            json!({"code": 0, "data": {"isLogin": "true"}}),
        ] {
            assert_eq!(
                validate_response(Ok(body)).unwrap_err().code,
                "VIDEO_API_INVALID"
            );
        }
    }

    /// 昵称与头像来自 nav，并限制长度，异常响应不能撑大前端状态。
    #[test]
    fn reads_profile_with_bounded_fields() {
        let data = json!({"code": 0, "data": {
            "isLogin": true, "uname": " 测试用户 ", "face": "http://i2.hdslb.com/bfs/face/x.jpg", "mid": 7
        }});
        let profile = validate_response(Ok(data)).unwrap();
        assert_eq!(profile.name, "测试用户");
        assert_eq!(profile.avatar, "http://i2.hdslb.com/bfs/face/x.jpg");
        let long = "a".repeat(600);
        let profile = validate_response(Ok(json!({
            "code": 0, "data": {"isLogin": true, "uname": long, "face": 7}
        })))
        .unwrap();
        assert_eq!(profile.name.chars().count(), 64);
        assert!(profile.avatar.is_empty());
    }

    /// 登录失效与业务拒绝、网络故障保留不同错误语义。
    #[test]
    fn distinguishes_invalid_login_from_api_and_network_errors() {
        for body in [
            json!({"code": -101}),
            json!({"code": 0, "data": {"isLogin": false}}),
        ] {
            assert_eq!(
                validate_response(Ok(body)).unwrap_err().code,
                "VIDEO_LOGIN_REQUIRED"
            );
        }
        let error = validate_response(Ok(json!({"code": -400, "message": "secret"}))).unwrap_err();
        assert_ne!(error.code, "VIDEO_LOGIN_REQUIRED");
        assert!(!error.message.contains("secret"));
        let error =
            validate_response(Err(CommandError::new("VIDEO_HTTP_ERROR", "网络失败"))).unwrap_err();
        assert_eq!(error.code, "VIDEO_HTTP_ERROR");
    }

    #[tokio::test]
    /// 完全重建存储与保险库服务后仍读取同一 B 站凭据，不依赖扫码内存会话。
    async fn restores_persisted_cookie_after_restart() {
        use crate::{
            storage::{testutil, Storage},
            vault::EncryptedVault,
            video_bridge::BILI_CREDENTIAL,
        };
        let (storage, config, _root) = testutil::test_storage().await;
        let vault = EncryptedVault::new();
        vault.initialize(&storage).await.unwrap();
        let raw =
            Zeroizing::new("SESSDATA=fake-session; bili_jct=fake-csrf; buvid4=device".to_string());
        let envelope = vault
            .prepare_set_credential(&storage, BILI_CREDENTIAL, &raw, None)
            .await
            .unwrap();
        storage.save_vault_envelope(&envelope).await.unwrap();
        drop(vault);
        drop(storage);
        let reopened = Storage::open(config.path()).unwrap();
        let restarted_vault = EncryptedVault::new();
        let jar = load(
            restarted_vault
                .get_credential(&reopened, BILI_CREDENTIAL)
                .await,
        )
        .unwrap();
        assert!(jar.is_logged_in());
        assert_eq!(jar.buvid4, "device");
    }

    /// 空凭据立即返回未登录，不依赖网络可达性。
    #[tokio::test]
    async fn anonymous_validation_skips_network() {
        let client = BiliClient::new(CookieJar::default()).unwrap();
        assert!(validate(&client).await.unwrap().is_none());
    }
}
