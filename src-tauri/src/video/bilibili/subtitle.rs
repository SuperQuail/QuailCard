//! 字幕获取：只有服务端明确返回空轨列表才表示缺失，网络与解析失败必须保留。
use super::{http::BiliClient, media, wbi};
use crate::{
    error::CommandError,
    video::{transcript, url::VideoRef},
};
use serde_json::Value;

const PRIORITY: [&str; 8] = [
    "ai-zh", "zh-hans", "zh-cn", "zh", "zh-hant", "ai-en", "en", "ai-ja",
];

/// 字幕接口同样要求当前 WBI 签名；av 输入通过 aid 参数而不是空 bvid 访问。
pub(crate) async fn fetch(
    client: &BiliClient,
    video: &VideoRef,
    cid: u64,
) -> Result<Option<(String, String)>, CommandError> {
    let keys = media::wbi_keys(client).await?;
    let params = identity(video, cid)?;
    let signed = keys.sign(&params, media::now_seconds());
    let url = format!(
        "https://api.bilibili.com/x/player/wbi/v2?{}",
        wbi::query_string(&signed)
    );
    let data = client.api(&url).await?;
    let candidates = candidates(&data)?;
    let mut failure = None;
    for (_, lan, address) in candidates {
        let result = async {
            let payload = client.text(&address, &video.page_url()).await?;
            let segments = transcript::parse_subtitle_json(&payload)?;
            if segments.is_empty() {
                return Err(invalid());
            }
            Ok(payload)
        }
        .await;
        match result {
            Ok(payload) => {
                let source = if lan.starts_with("ai-") {
                    "bilibili_ai"
                } else {
                    "bilibili_cc"
                };
                return Ok(Some((source.to_string(), payload)));
            }
            Err(error) => {
                failure = Some(error);
            }
        }
    }
    match failure {
        Some(error) => Err(error),
        None => Ok(None),
    }
}

/// 拒绝未解析身份，防止 API 错误被错误解释为没有字幕。
fn identity(video: &VideoRef, cid: u64) -> Result<Vec<(String, String)>, CommandError> {
    if cid == 0 {
        return Err(invalid());
    }
    let id = if !video.bvid.is_empty() {
        ("bvid".to_string(), video.bvid.clone())
    } else {
        (
            "aid".to_string(),
            video
                .aid
                .filter(|aid| *aid > 0)
                .ok_or_else(invalid)?
                .to_string(),
        )
    };
    Ok(vec![id, ("cid".to_string(), cid.to_string())])
}

/// 列表必须有效，非空列表的畸形轨不能伪装为字幕缺失。
fn candidates(data: &Value) -> Result<Vec<(usize, String, String)>, CommandError> {
    let tracks = data
        .get("subtitle")
        .and_then(|value| value.get("subtitles"))
        .and_then(Value::as_array)
        .ok_or_else(invalid)?;
    let mut result = Vec::new();
    for track in tracks {
        let lan = track
            .get("lan")
            .and_then(Value::as_str)
            .ok_or_else(invalid)?
            .to_lowercase();
        let raw = track
            .get("subtitle_url")
            .and_then(Value::as_str)
            .filter(|s| !s.is_empty())
            .ok_or_else(invalid)?;
        let address = if raw.starts_with("//") {
            format!("https:{raw}")
        } else {
            raw.to_string()
        };
        super::targets::validate(&address)?;
        let rank = PRIORITY
            .iter()
            .position(|item| *item == lan)
            .unwrap_or(PRIORITY.len());
        result.push((rank, lan, address));
    }
    result.sort_by_key(|(rank, _, _)| *rank);
    Ok(result)
}

/// 不包含原始字幕或签名地址，调用方可安全展示。
fn invalid() -> CommandError {
    CommandError::new("VIDEO_SUBTITLE_INVALID", "B 站字幕响应无效，请稍后重试")
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    /// 明确空列表才是缺失，结构损坏与危险资源必须报错。
    fn absence_is_distinct_from_invalid_response() {
        assert!(candidates(&json!({"subtitle":{"subtitles":[]}}))
            .unwrap()
            .is_empty());
        for data in [
            json!(null),
            json!({}),
            json!({"subtitle":{"subtitles":[{}]}}),
            json!({"subtitle":{"subtitles":[{"lan":"zh", "subtitle_url":"https://evil.test/secret"}]}}),
        ] {
            assert!(candidates(&data).is_err());
        }
    }

    #[test]
    /// 协议相对 CDN 地址规范化，并优先中文 AI 轨。
    fn ranks_valid_tracks() {
        let tracks = candidates(&json!({"subtitle":{"subtitles":[
            {"lan":"en","subtitle_url":"https://i0.hdslb.com/en.json"},
            {"lan":"ai-zh","subtitle_url":"//aisubtitle.hdslb.com/zh.json"}]}}))
        .unwrap();
        assert_eq!(tracks[0].1, "ai-zh");
        assert_eq!(tracks[0].2, "https://aisubtitle.hdslb.com/zh.json");
    }

    #[test]
    /// av 通过 aid 查询，不发送空 BV；分 P 的 cid 不能丢失。
    fn av_identity_uses_aid() {
        let crate::video::url::VideoInput::Video(video) =
            crate::video::url::parse("av123").unwrap()
        else {
            panic!("需要视频身份")
        };
        assert_eq!(
            identity(&video, 456).unwrap(),
            vec![("aid".into(), "123".into()), ("cid".into(), "456".into())]
        );
        assert!(identity(&video, 0).is_err());
    }
}
