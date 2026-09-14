//! 头像代理：把 B 站图床图片转成受限大小的 data URL，避免 webview 直连第三方 CDN。

use base64::{engine::general_purpose::STANDARD, Engine as _};
use reqwest::header::CONTENT_TYPE;

use super::http::{BiliClient, REFERER};
use crate::error::CommandError;

/// 头像上限；正常头像远小于该值，超出说明拿到的不是头像资源。
const MAX_AVATAR_BYTES: usize = 512 * 1024;

/// 只允许这几种图片类型进入界面，避免把错误页或脚本当成头像。
const IMAGE_TYPES: [&str; 4] = ["image/png", "image/jpeg", "image/webp", "image/gif"];

/// 读取头像并返回 data URL；地址只允许官方图床的 HTTPS 资源。
pub(crate) async fn fetch(client: &BiliClient, face: &str) -> Result<String, CommandError> {
    let url = secure_url(face)?;
    let mut response = client.stream(&url, REFERER).await?;
    let mime = image_type(
        response
            .headers()
            .get(CONTENT_TYPE)
            .and_then(|value| value.to_str().ok()),
    )
    .ok_or_else(unavailable)?;
    let mut bytes = Vec::new();
    while let Some(chunk) = response.chunk().await.map_err(|_| unavailable())? {
        if bytes.len() + chunk.len() > MAX_AVATAR_BYTES {
            return Err(unavailable());
        }
        bytes.extend_from_slice(&chunk);
    }
    if bytes.is_empty() {
        return Err(unavailable());
    }
    Ok(format!("data:{mime};base64,{}", STANDARD.encode(bytes)))
}

/// 只接受官方图床主机，并把接口可能返回的 http 地址升级为 https。
fn secure_url(face: &str) -> Result<String, CommandError> {
    let candidate = face.trim();
    if candidate.is_empty() || candidate.len() > 512 || candidate.chars().any(char::is_control) {
        return Err(unavailable());
    }
    let upgraded = match candidate.strip_prefix("http://") {
        Some(rest) => format!("https://{rest}"),
        None => candidate.to_string(),
    };
    let parsed = super::targets::validate(&upgraded).map_err(|_| unavailable())?;
    let host = parsed.host_str().unwrap_or_default();
    if host != "hdslb.com" && !host.ends_with(".hdslb.com") {
        return Err(unavailable());
    }
    Ok(upgraded)
}

/// 响应类型必须先归一化为白名单取值，再拼进 data URL。
fn image_type(raw: Option<&str>) -> Option<&'static str> {
    let value = raw?.split(';').next()?.trim().to_ascii_lowercase();
    IMAGE_TYPES.iter().copied().find(|item| *item == value)
}

/// 头像不可用时只给安全提示，不回显地址或响应内容。
fn unavailable() -> CommandError {
    CommandError::new("VIDEO_AVATAR_UNAVAILABLE", "无法读取 B 站头像")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    /// 图床地址必须升级为 HTTPS，且不能指向任意主机。
    fn only_official_https_hosts() {
        assert_eq!(
            secure_url("http://i2.hdslb.com/bfs/face/x.jpg").unwrap(),
            "https://i2.hdslb.com/bfs/face/x.jpg"
        );
        assert!(secure_url("https://i0.hdslb.com/bfs/face/x.png").is_ok());
        for face in [
            "",
            "https://evil.test/face.jpg",
            "https://hdslb.com.evil.test/x.jpg",
            "https://127.0.0.1/x.jpg",
            "file:///etc/passwd",
            "https://i2.hdslb.com/x.jpg\r\nX-Injected: 1",
        ] {
            assert_eq!(
                secure_url(face).unwrap_err().code,
                "VIDEO_AVATAR_UNAVAILABLE"
            );
        }
    }

    #[test]
    /// 只有白名单图片类型可用于 data URL，其余响应一律拒绝。
    fn whitelists_image_types() {
        assert_eq!(image_type(Some("image/jpeg")), Some("image/jpeg"));
        assert_eq!(
            image_type(Some("IMAGE/PNG; charset=binary")),
            Some("image/png")
        );
        assert_eq!(image_type(Some("text/html")), None);
        assert_eq!(image_type(Some("image/svg+xml")), None);
        assert_eq!(image_type(None), None);
    }
}
