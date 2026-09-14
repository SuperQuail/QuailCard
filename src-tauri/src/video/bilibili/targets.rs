//! 请求目的地策略：每次跳转重新校验，凭据只允许进入固定主站及认证主机。
use crate::error::CommandError;
use reqwest::Url;

/// 仅接受 HTTPS 与默认端口；B 站自有 mcdn 的 8082 是无凭据媒体专用例外。
pub(super) fn validate(raw: &str) -> Result<Url, CommandError> {
    let url = Url::parse(raw).map_err(|_| invalid())?;
    if url.scheme() != "https"
        || !url.username().is_empty()
        || url.password().is_some()
        || raw.chars().any(char::is_control)
        || raw.contains('\\')
    {
        return Err(invalid());
    }
    let host = url.host_str().ok_or_else(invalid)?;
    if url.port().is_some() && !(url.port() == Some(8082) && host.ends_with(".mcdn.bilivideo.cn")) {
        return Err(invalid());
    }
    if !matches!(
        host,
        "bilibili.com"
            | "www.bilibili.com"
            | "m.bilibili.com"
            | "api.bilibili.com"
            | "passport.bilibili.com"
            | "account.bilibili.com"
            | "b23.tv"
    ) && !["bilivideo.com", "bilivideo.cn", "hdslb.com"]
        .iter()
        .any(|domain| host == *domain || host.ends_with(&format!(".{domain}")))
    {
        return Err(invalid());
    }
    Ok(url)
}

/// CDN、短链及任意子域均不能发送或接收登录凭据。
pub(super) fn credentials(url: &Url) -> bool {
    url.scheme() == "https"
        && url.port().is_none()
        && url.username().is_empty()
        && url.password().is_none()
        && matches!(
            url.host_str(),
            Some(
                "bilibili.com" | "www.bilibili.com" | "api.bilibili.com" | "passport.bilibili.com"
            )
        )
}

/// 页面跳转只允许官方页面域，不能把短链变为 CDN 或认证请求。
/// account.bilibili.com 是当前扫码页主机，只作为页面域，不接收会话凭据。
pub(super) fn page(url: &Url) -> bool {
    matches!(
        url.host_str(),
        Some(
            "bilibili.com"
                | "www.bilibili.com"
                | "m.bilibili.com"
                | "account.bilibili.com"
                | "b23.tv"
        )
    )
}

/// 安全校验不回显签名查询、二维码密钥或恶意地址。
pub(super) fn invalid() -> CommandError {
    CommandError::new("VIDEO_URL_INVALID", "B 站资源地址不安全或不受支持")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    /// 精确域名、协议和端口规则必须抵抗相似域及用户信息绕过。
    fn rejects_unsafe_targets() {
        for url in [
            "http://api.bilibili.com/",
            "file:///secret",
            "https://bilibili.com.evil.test/",
            "https://evil.bilibili.com/",
            "https://api.bilibili.com:8443/",
            "https://u:p@api.bilibili.com/",
            "https://127.0.0.1/",
            "https://evilbilivideo.com/",
            "https://api.bilibili.com/\\secret",
        ] {
            let error = validate(url).unwrap_err();
            assert_eq!(error.code, "VIDEO_URL_INVALID");
            assert!(!error.message.contains("secret"));
        }
    }

    #[test]
    /// 媒体专用端口仅对自有 mcdn 子域有效，主站和第三方均不能借此越界。
    fn permits_only_official_mcdn_media_port_without_credentials() {
        let media = validate("https://xy120x222x159x40xy.mcdn.bilivideo.cn:8082/video").unwrap();
        assert!(!credentials(&media));
        assert!(!page(&media));
        for target in [
            "https://api.bilibili.com:8082/",
            "https://upos.bilivideo.com:8082/",
            "https://xy.mcdn.bilivideo.cn:4483/",
            "https://xy.mcdn.bilivideo.cn:8443/",
            "http://xy.mcdn.bilivideo.cn:8082/",
            "https://user:secret@xy.mcdn.bilivideo.cn:8082/",
            "https://evilmcdn.bilivideo.cn:8082/",
            "https://xy.mcdn.bilivideo.cn.evil.test:8082/",
            "https://edge.mountaintoys.cn:4483/",
            "https://cdn.v.smtcdns.com/",
        ] {
            assert!(validate(target).is_err());
        }
    }

    #[test]
    /// 扫码页迁移到 account 子域后仍按官方页面处理，但绝不携带会话凭据。
    fn accepts_account_page_without_credentials() {
        let url = validate("https://account.bilibili.com/h5/account-h5/auth/scan-web?qrcode_key=a")
            .unwrap();
        assert!(page(&url));
        assert!(!credentials(&url));
    }

    #[test]
    /// 允许媒体域访问但永远不携带会话，短链同样隔离。
    fn separates_resource_and_credential_hosts() {
        for host in [
            "b23.tv",
            "m.bilibili.com",
            "upos-sz.bilivideo.com",
            "i0.hdslb.com",
            "cn.bilivideo.cn",
        ] {
            assert!(!credentials(
                &validate(&format!("https://{host}/")).unwrap()
            ));
        }
        for host in [
            "bilibili.com",
            "www.bilibili.com",
            "api.bilibili.com",
            "passport.bilibili.com",
        ] {
            assert!(credentials(&validate(&format!("https://{host}/")).unwrap()));
        }
    }
}
