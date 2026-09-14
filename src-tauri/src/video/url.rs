//! B 站链接规范化：只接收官方页面地址及完整裸视频号。
use crate::error::CommandError;

#[derive(Debug, Clone, PartialEq)]
pub(crate) enum VideoInput {
    Video(VideoRef),
    ShortLink(String),
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct VideoRef {
    pub bvid: String,
    pub aid: Option<u64>,
    pub page: Option<u32>,
    pub source_url: String,
    pub cache_key: String,
}

impl VideoRef {
    /// 来源固定为官方站点，不能从外部字符串拼接 Referer 主机。
    pub(crate) fn page_url(&self) -> String {
        let id = if self.bvid.is_empty() {
            format!("av{}", self.aid.unwrap_or_default())
        } else {
            self.bvid.clone()
        };
        format!("https://www.bilibili.com/video/{id}")
    }
}

/// 分享文本只提取 HTTP(S) 地址；没有链接时必须是完整裸号或裸官方地址。
pub(crate) fn parse(input: &str) -> Result<VideoInput, CommandError> {
    let text = input.trim();
    if text.is_empty() || text.chars().count() > 2000 {
        return Err(invalid());
    }
    let lower = text.to_ascii_lowercase();
    let start = [lower.find("http://"), lower.find("https://")]
        .into_iter()
        .flatten()
        .min();
    let candidate = if let Some(start) = start {
        // 不允许把恶意 scheme 的内嵌 HTTP 片段当作分享链接。
        if start > 0 && !text[..start].ends_with(|c: char| c.is_whitespace() || "（【(".contains(c))
        {
            return Err(invalid());
        }
        text[start..]
            .split(|c: char| c.is_whitespace() || "。，、）】)".contains(c))
            .next()
            .unwrap_or("")
    } else {
        text
    };
    if valid_id(candidate).is_some() {
        return build(candidate, None);
    }
    let address = if candidate.contains("://") {
        candidate.to_string()
    } else {
        format!("https://{candidate}")
    };
    let mut url = page_address(&address)?;
    url.set_scheme("https").map_err(|_| invalid())?;
    if url.host_str() == Some("b23.tv") {
        url.set_fragment(None);
        return Ok(VideoInput::ShortLink(url.to_string()));
    }
    from_page(&url)
}

/// 跳转结果必须是完整官方视频页，不能接受裸号、短链或其他协议。
pub(crate) fn parse_resolved(raw: &str) -> Result<VideoInput, CommandError> {
    let url = page_address(raw)?;
    if url.scheme() != "https" || url.host_str() == Some("b23.tv") {
        return Err(invalid());
    }
    from_page(&url)
}

/// URL 库负责 authority 解析，拒绝用户信息、非默认端口与反斜杠混淆。
fn page_address(raw: &str) -> Result<reqwest::Url, CommandError> {
    if raw.contains('\\') || raw.chars().any(char::is_control) {
        return Err(invalid());
    }
    let url = reqwest::Url::parse(raw).map_err(|_| invalid())?;
    if !matches!(url.scheme(), "http" | "https")
        || !url.username().is_empty()
        || url.password().is_some()
        || url.port().is_some()
        || !matches!(
            url.host_str(),
            Some("bilibili.com" | "www.bilibili.com" | "m.bilibili.com" | "b23.tv")
        )
    {
        return Err(invalid());
    }
    Ok(url)
}

/// 视频号只能位于 video 路径段，查询与片段中的假视频号不能被识别。
fn from_page(url: &reqwest::Url) -> Result<VideoInput, CommandError> {
    let path = url.path().trim_end_matches('/');
    let id = path.strip_prefix("/video/").ok_or_else(invalid)?;
    let page = url
        .query_pairs()
        .find(|(key, _)| key == "p")
        .and_then(|(_, value)| value.parse::<u32>().ok())
        .filter(|value| *value > 0);
    build(id, page)
}

/// 要求 BV 长度精确或非零十进制 av，避免从任意文本截取身份。
fn valid_id(id: &str) -> Option<(String, Option<u64>)> {
    if id.len() == 12 && id.starts_with("BV") && id[2..].bytes().all(|c| c.is_ascii_alphanumeric())
    {
        return Some((id.to_string(), None));
    }
    if id
        .get(..2)
        .is_some_and(|prefix| prefix.eq_ignore_ascii_case("av"))
    {
        let digits = &id[2..];
        if !digits.is_empty() && digits.bytes().all(|c| c.is_ascii_digit()) {
            return digits
                .parse::<u64>()
                .ok()
                .filter(|aid| *aid > 0)
                .map(|aid| (String::new(), Some(aid)));
        }
    }
    None
}

/// 不保留原始查询秘密，分 P 参与规范地址和缓存键。
fn build(id: &str, page: Option<u32>) -> Result<VideoInput, CommandError> {
    let (bvid, aid) = valid_id(id).ok_or_else(invalid)?;
    let key_id = aid
        .map(|aid| format!("av{aid}"))
        .unwrap_or_else(|| bvid.clone());
    let mut source_url = format!("https://www.bilibili.com/video/{key_id}");
    if let Some(page) = page {
        source_url.push_str(&format!("?p={page}"));
    }
    Ok(VideoInput::Video(VideoRef {
        bvid,
        aid,
        page,
        source_url,
        cache_key: format!("bilibili:{key_id}:p{}", page.unwrap_or(1)),
    }))
}

/// 输入错误不回显可能含有凭据的分享文本。
fn invalid() -> CommandError {
    CommandError::new(
        "VIDEO_URL_INVALID",
        "请输入有效的 B 站视频链接或 BV / av 号",
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 取出视频身份，短链在此处视为失败。
    fn video(input: &str) -> VideoRef {
        match parse(input).unwrap() {
            VideoInput::Video(video) => video,
            VideoInput::ShortLink(_) => panic!("不应识别为短链"),
        }
    }

    #[test]
    /// 完整链接保留分 P 并生成复用键。
    fn parses_full_url_with_page() {
        let parsed = video("https://www.bilibili.com/video/BV1Qwby6DEu1/?p=3&t=10");
        assert_eq!(parsed.bvid, "BV1Qwby6DEu1");
        assert_eq!(parsed.page, Some(3));
        assert_eq!(parsed.cache_key, "bilibili:BV1Qwby6DEu1:p3");
        assert_eq!(
            parsed.source_url,
            "https://www.bilibili.com/video/BV1Qwby6DEu1?p=3"
        );
        assert_eq!(
            parsed.page_url(),
            "https://www.bilibili.com/video/BV1Qwby6DEu1"
        );
    }

    #[test]
    /// 分享文本与裸号都能识别，裸号回落到官方站点。
    fn parses_share_text_and_bare_id() {
        let shared = video("【视频】 https://www.bilibili.com/video/BV1Qwby6DEu1/ 分享自 B 站");
        assert_eq!(shared.bvid, "BV1Qwby6DEu1");
        let bare = video("BV1Qwby6DEu1");
        assert_eq!(
            bare.source_url,
            "https://www.bilibili.com/video/BV1Qwby6DEu1"
        );
        let av = video("av12345");
        assert_eq!(av.aid, Some(12345));
        assert_eq!(av.cache_key, "bilibili:av12345:p1");
    }

    #[test]
    /// 短链交给网络跳转阶段处理，含省略协议头的写法。
    fn detects_short_link() {
        assert!(matches!(
            parse("b23.tv/abc123"),
            Ok(VideoInput::ShortLink(_))
        ));
        assert!(matches!(
            parse("看这个 https://b23.tv/abc123"),
            Ok(VideoInput::ShortLink(_))
        ));
    }

    #[test]
    /// 协议、用户信息、相似域及片段都不能绕过完整路径身份校验。
    fn rejects_confused_urls_and_identifiers() {
        for value in [
            "ftp://www.bilibili.com/video/av1",
            "javascript:https://www.bilibili.com/video/av1",
            "https://u:p@www.bilibili.com/video/av1",
            "https://www.bilibili.com:8443/video/av1",
            "https://www.bilibili.com.evil.test/video/av1",
            "https://evil.bilibili.com/video/av1",
            "https://www.bilibili.com/?next=/video/av1",
            "https://www.bilibili.com/#/video/av1",
            "https://www.bilibili.com/video/av1/extra",
            "BV1Qwby6DEu1x",
            "av0",
            "文字av1",
        ] {
            assert!(parse(value).is_err(), "非法输入被接受");
        }
        assert!(parse_resolved("av1").is_err());
        assert!(parse_resolved("https://b23.tv/av1").is_err());
        assert_eq!(
            video("HTTP://WWW.BILIBILI.COM/video/av1?p=2#secret").page,
            Some(2)
        );
    }

    #[test]
    /// 非 B 站链接与无法识别的文本明确拒绝。
    fn rejects_other_hosts_and_text() {
        assert_eq!(
            parse("https://www.youtube.com/watch?v=abc")
                .unwrap_err()
                .code,
            "VIDEO_URL_INVALID"
        );
        assert_eq!(parse("随便一段文字").unwrap_err().code, "VIDEO_URL_INVALID");
        assert_eq!(parse("save123").unwrap_err().code, "VIDEO_URL_INVALID");
    }
}
