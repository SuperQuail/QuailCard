//! 元信息解析：av 经 aid 请求并用服务端 BV 身份完成解析。
use super::{
    http::BiliClient,
    media::{PageInfo, VideoInfo},
};
use crate::{error::CommandError, video::url::VideoRef};
use serde_json::Value;

/// 读取视频元信息与全部分 P。
pub(crate) async fn video_info(
    client: &BiliClient,
    video: &VideoRef,
) -> Result<VideoInfo, CommandError> {
    let url = endpoint(video)?;
    let data = client.api(&url).await?;
    parse_info(&data, video)
}

/// 查询参数由已校验身份编码，av 输入不能退化为空 BV 或 aid=0。
fn endpoint(video: &VideoRef) -> Result<String, CommandError> {
    if video.bvid.is_empty() && video.aid.is_none_or(|aid| aid == 0) {
        return Err(invalid());
    }
    let url = if video.bvid.is_empty() {
        format!(
            "https://api.bilibili.com/x/web-interface/view?aid={}",
            video.aid.unwrap_or_default()
        )
    } else {
        format!(
            "https://api.bilibili.com/x/web-interface/view?bvid={}",
            super::wbi::query_string(&[("bvid".into(), video.bvid.clone())])
                .trim_start_matches("bvid=")
        )
    };
    Ok(url)
}

/// 服务端必须返回有效 BV 和 aid，缺失身份不能继续签名或缓存。
fn parse_info(data: &Value, video: &VideoRef) -> Result<VideoInfo, CommandError> {
    let aid = data
        .get("aid")
        .and_then(Value::as_u64)
        .filter(|aid| *aid > 0)
        .ok_or_else(invalid)?;
    let bvid = data
        .get("bvid")
        .and_then(Value::as_str)
        .ok_or_else(invalid)?;
    if bvid.len() != 12
        || !bvid.starts_with("BV")
        || !bvid[2..].bytes().all(|byte| byte.is_ascii_alphanumeric())
        || video.aid.is_some_and(|expected| expected != aid)
        || (!video.bvid.is_empty() && video.bvid != bvid)
    {
        return Err(invalid());
    }
    let mut pages = Vec::new();
    if let Some(items) = data.get("pages").and_then(Value::as_array) {
        for item in items {
            pages.push(PageInfo {
                page: item.get("page").and_then(Value::as_u64).unwrap_or(1) as u32,
                title: item
                    .get("part")
                    .and_then(Value::as_str)
                    .unwrap_or("")
                    .to_string(),
                cid: item.get("cid").and_then(Value::as_u64).unwrap_or(0),
                duration: item.get("duration").and_then(Value::as_f64).unwrap_or(0.0),
            });
        }
    }
    if pages.iter().any(|page| page.cid == 0 || page.page == 0) {
        return Err(invalid());
    }
    if pages.is_empty() {
        return Err(CommandError::new(
            "VIDEO_NOT_FOUND",
            "接口未返回可用的分 P 信息",
        ));
    }
    Ok(VideoInfo {
        aid,
        bvid: bvid.to_string(),
        title: data
            .get("title")
            .and_then(Value::as_str)
            .unwrap_or("B 站视频")
            .to_string(),
        owner: data
            .get("owner")
            .and_then(|owner| owner.get("name"))
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string(),
        duration: data.get("duration").and_then(Value::as_f64).unwrap_or(0.0),
        pages,
    })
}

/// 元信息错误只保留固定说明，不回显响应或 URL。
fn invalid() -> CommandError {
    CommandError::new("VIDEO_API_INVALID", "B 站返回的视频身份或分 P 信息无效")
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    /// av 只用 aid 查询，响应必须给出对应 BV 后才能继续。
    fn resolves_av_with_aid() {
        let crate::video::url::VideoInput::Video(video) =
            crate::video::url::parse("av123").unwrap()
        else {
            panic!("需要视频身份")
        };
        assert_eq!(
            endpoint(&video).unwrap(),
            "https://api.bilibili.com/x/web-interface/view?aid=123"
        );
        let data = json!({"aid":123,"bvid":"BV1Qwby6DEu1","pages":[{"page":1,"cid":456}]});
        assert_eq!(parse_info(&data, &video).unwrap().bvid, "BV1Qwby6DEu1");
        assert!(parse_info(&json!({"aid":123,"pages":[{"cid":456}]}), &video).is_err());
        let mut mismatch = data;
        mismatch["aid"] = json!(999);
        assert!(parse_info(&mismatch, &video).is_err());
    }
}
