//! 安全 HTTP 诊断：只输出固定接口标签、返回码与凭据是否携带，不泄漏 URL 参数。
use crate::error::CommandError;

/// 只对白名单接口展示路径；其他地址不回显主机、路径或查询参数。
pub(crate) fn endpoint(url: &str) -> &'static str {
    let Ok(url) = reqwest::Url::parse(url) else {
        return "资源请求";
    };
    let trusted = matches!(
        url.host_str(),
        Some("api.bilibili.com" | "passport.bilibili.com")
    );
    if !trusted {
        return "资源请求";
    }
    match url.path() {
        "/x/web-interface/view" => "视频信息 /x/web-interface/view",
        "/x/web-interface/nav" => "登录校验 /x/web-interface/nav",
        "/x/player/wbi/playurl" => "播放地址 /x/player/wbi/playurl",
        "/x/player/wbi/v2" => "字幕 /x/player/wbi/v2",
        "/x/passport-login/web/qrcode/generate" => "二维码申请",
        "/x/passport-login/web/qrcode/poll" => "扫码确认",
        _ => "B 站接口",
    }
}

/// 仅描述请求是否携带凭据，不能把本地有 Cookie 说成已通过校验。
fn credential_hint(has_cookie: bool) -> &'static str {
    if has_cookie {
        "已携带登录凭据，不代表服务端校验通过"
    } else {
        "未携带登录凭据"
    }
}

/// 同时保留 HTTP 层返回码和接口定位；412 只能称为疑似风控。
pub(crate) fn http_error(status: u16, url: &str, has_cookie: bool) -> CommandError {
    let (code, message) = match status {
        412 => ("VIDEO_RISK_CONTROL", "请求被拒绝（疑似风控），请稍后重试"),
        403 => ("VIDEO_FORBIDDEN", "请求被拒绝"),
        404 => ("VIDEO_NOT_FOUND", "视频或资源不存在"),
        429 => ("VIDEO_RATE_LIMITED", "请求过于频繁，请稍后重试"),
        _ => ("VIDEO_HTTP_ERROR", "HTTP 请求异常"),
    };
    CommandError::new(
        code,
        format!(
            "{}：{message}（HTTP {status}；{}）",
            endpoint(url),
            credential_hint(has_cookie)
        ),
    )
}

/// 业务码保留原值；不把所有错误都归为风控，不回显服务端原始消息。
pub(crate) fn api_error(code: i64, url: &str, has_cookie: bool) -> CommandError {
    let (kind, message) = match code {
        -101 => ("VIDEO_LOGIN_REQUIRED", "B 站登录未生效或已失效，请重新扫码"),
        -403 => ("VIDEO_FORBIDDEN", "没有访问该视频的权限"),
        -404 => ("VIDEO_NOT_FOUND", "视频不存在或已删除"),
        -352 | -412 => (
            "VIDEO_RISK_CONTROL",
            "请求被安全校验拒绝（可能与会话、签名或风控有关）",
        ),
        _ => ("VIDEO_API_FAILED", "接口返回异常"),
    };
    CommandError::new(
        kind,
        format!(
            "{}：{message}（业务码 {code}；{}）",
            endpoint(url),
            credential_hint(has_cookie)
        ),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    /// HTTP 与业务码必须区分，登录后也不能使用笼统的「请先登录」。
    fn precise_status_and_auth_context() {
        let url = "https://api.bilibili.com/x/player/wbi/playurl?w_rid=secret&SESSDATA=secret";
        let http = http_error(412, url, true);
        assert!(http.message.contains("HTTP 412"));
        assert!(http.message.contains("已携带登录凭据"));
        assert!(http.message.contains("播放地址"));
        assert!(!http.message.contains("secret"));
        let api = api_error(-352, url, false);
        assert!(api.message.contains("业务码 -352"));
        assert!(api.message.contains("未携带登录凭据"));
        assert_eq!(api_error(-101, url, true).code, "VIDEO_LOGIN_REQUIRED");
        assert_eq!(api_error(-400, url, true).code, "VIDEO_API_FAILED");
        assert_eq!(http_error(403, url, true).code, "VIDEO_FORBIDDEN");
        assert_eq!(http_error(429, url, true).code, "VIDEO_RATE_LIMITED");
    }

    #[test]
    /// 二维码密钥和未知资源地址永不进入诊断消息。
    fn redacts_untrusted_urls() {
        assert_eq!(
            endpoint(
                "https://passport.bilibili.com/x/passport-login/web/qrcode/poll?qrcode_key=secret"
            ),
            "扫码确认"
        );
        assert_eq!(
            endpoint("https://secret@other.invalid/private-token"),
            "资源请求"
        );
        assert_eq!(endpoint("not a url secret"), "资源请求");
    }
}
