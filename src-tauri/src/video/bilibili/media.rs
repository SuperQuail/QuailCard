//! B 站视频信息、字幕轨与 DASH 媒体地址。

use serde_json::Value;

use super::{
    http::BiliClient,
    wbi::{self, WbiKeys},
};
use crate::{error::CommandError, video::url::VideoRef};

/// 单个分 P。
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct PageInfo {
    pub page: u32,
    pub title: String,
    pub cid: u64,
    pub duration: f64,
}

/// 视频元信息。
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct VideoInfo {
    pub aid: u64,
    pub bvid: String,
    pub title: String,
    pub owner: String,
    pub duration: f64,
    pub pages: Vec<PageInfo>,
}

/// 一条 DASH 视频轨。
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct VideoTrack {
    pub qn: u32,
    pub codec: u32,
    pub height: u32,
    pub bandwidth: u64,
    pub url: String,
    pub backup: Vec<String>,
}

/// 清晰度选项：面向界面展示，不直接暴露地址。
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct QualityOption {
    pub qn: u32,
    pub label: String,
    pub height: u32,
    pub available: bool,
    pub requires_vip: bool,
    pub estimated_bytes: u64,
    pub estimated_duration: f64,
    pub unavailable_reason: String,
}

/// 一次播放地址查询的结果。
#[derive(Debug, Clone, Default, PartialEq)]
pub(crate) struct MediaTracks {
    pub audio: Vec<String>,
    pub videos: Vec<VideoTrack>,
}

/// 播放地址与面向界面的清晰度选项。
#[derive(Debug, Clone, Default, PartialEq)]
pub(crate) struct PlayResult {
    pub tracks: MediaTracks,
    pub qualities: Vec<QualityOption>,
}

/// 展开 b23.tv 短链；跳转结果仍限定 B 站域名。
pub(crate) async fn resolve_short_link(
    client: &BiliClient,
    url: &str,
) -> Result<String, CommandError> {
    let response = client.stream(url, "").await?;
    crate::video::url::parse_resolved(response.url().as_str())?;
    Ok(response.url().to_string())
}

pub(crate) use super::metadata::video_info;

/// 读取当前密钥；不得吞掉 nav 错误并使用过时密钥，制造下游签名错误。
pub(crate) async fn wbi_keys(client: &BiliClient) -> Result<WbiKeys, CommandError> {
    let url = "https://api.bilibili.com/x/web-interface/nav";
    let envelope = client.envelope(url).await?;
    let code = envelope.get("code").and_then(Value::as_i64);
    // 匿名 nav 可返回 -101 但仍含公共密钥；携带凭据时则必须报告失效。
    if code != Some(0) && !(code == Some(-101) && !client.cookie().is_logged_in()) {
        return Err(super::http::api_error(
            code.unwrap_or(-1),
            url,
            client.cookie().is_logged_in(),
        ));
    }
    WbiKeys::from_nav(&envelope["data"])
}

/// 查询播放地址：带 WBI 签名，返回音频与视频候选轨。
pub(crate) async fn playurl(
    client: &BiliClient,
    keys: &WbiKeys,
    aid: u64,
    cid: u64,
    duration: f64,
) -> Result<PlayResult, CommandError> {
    playurl_quality(client, keys, aid, cid, duration, None).await
}

/// 请求实际质量档位；保持旧入口默认兼容，供下载前重新协商授权档位。
pub(crate) async fn playurl_quality(
    client: &BiliClient,
    keys: &WbiKeys,
    aid: u64,
    cid: u64,
    duration: f64,
    quality: Option<u32>,
) -> Result<PlayResult, CommandError> {
    let params = vec![
        ("avid".to_string(), aid.to_string()),
        ("cid".to_string(), cid.to_string()),
        ("fnval".to_string(), "4048".to_string()),
        ("fnver".to_string(), "0".to_string()),
        ("fourk".to_string(), "1".to_string()),
        ("qn".to_string(), quality.unwrap_or(32).to_string()),
        ("otype".to_string(), "json".to_string()),
    ];
    let signed = keys.sign(&params, now_seconds());
    let url = format!(
        "https://api.bilibili.com/x/player/wbi/playurl?{}",
        wbi::query_string(&signed)
    );
    let data = client.api(&url).await?;
    let dash = data.get("dash").ok_or_else(|| {
        CommandError::new(
            "VIDEO_NO_MEDIA",
            "该视频没有可用的媒体流（可能是会员专享或 DRM 保护）",
        )
    })?;
    let mut audio = Vec::new();
    if let Some(tracks) = dash.get("audio").and_then(Value::as_array) {
        let mut candidates: Vec<(u64, String, Vec<String>)> = tracks
            .iter()
            .filter_map(|track| {
                let codec = track.get("codecs").and_then(Value::as_str).unwrap_or("");
                let url = track.get("baseUrl").and_then(Value::as_str)?.to_string();
                let backup = strings(track.get("backupUrl"));
                let bandwidth = track.get("bandwidth").and_then(Value::as_u64).unwrap_or(0);
                let rank = if codec.starts_with("mp4a") { 1 } else { 0 };
                Some((rank * 1_000_000_000 + bandwidth, url, backup))
            })
            .collect();
        candidates.sort_by_key(|item| std::cmp::Reverse(item.0));
        for (_, url, backup) in candidates {
            audio.push(url);
            audio.extend(backup);
        }
    }
    let mut videos = Vec::new();
    if let Some(tracks) = dash.get("video").and_then(Value::as_array) {
        for track in tracks {
            let Some(url) = track.get("baseUrl").and_then(Value::as_str) else {
                continue;
            };
            videos.push(VideoTrack {
                qn: track.get("id").and_then(Value::as_u64).unwrap_or(0) as u32,
                codec: track.get("codecid").and_then(Value::as_u64).unwrap_or(0) as u32,
                height: track.get("height").and_then(Value::as_u64).unwrap_or(0) as u32,
                bandwidth: track.get("bandwidth").and_then(Value::as_u64).unwrap_or(0),
                url: url.to_string(),
                backup: strings(track.get("backupUrl")),
            });
        }
    }
    if audio.is_empty() && videos.is_empty() {
        return Err(CommandError::new(
            "VIDEO_NO_MEDIA",
            "该视频没有可下载的媒体流",
        ));
    }
    let qualities = quality_options(&data, &videos, duration);
    Ok(PlayResult {
        tracks: MediaTracks { audio, videos },
        qualities,
    })
}

/// 生成清晰度选项：只保留接口允许的档位并标注会员限制。
pub(crate) fn quality_options(
    data: &Value,
    tracks: &[VideoTrack],
    duration: f64,
) -> Vec<QualityOption> {
    let formats = data
        .get("support_formats")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let mut options = Vec::new();
    for format in formats {
        let qn = format.get("quality").and_then(Value::as_u64).unwrap_or(0) as u32;
        if qn == 0 {
            continue;
        }
        let track = tracks
            .iter()
            .filter(|track| track.qn == qn)
            .max_by_key(|track| track.bandwidth);
        let label = format
            .get("new_description")
            .and_then(Value::as_str)
            .or_else(|| format.get("display_desc").and_then(Value::as_str))
            .unwrap_or("未知清晰度")
            .to_string();
        let superscript = format
            .get("superscript")
            .and_then(Value::as_str)
            .unwrap_or("");
        let estimated = track
            .map(|track| (track.bandwidth as f64 * duration / 8.0) as u64)
            .unwrap_or(0);
        options.push(QualityOption {
            qn,
            label,
            height: track.map(|track| track.height).unwrap_or(0),
            available: track.is_some(),
            requires_vip: superscript.contains('会') || superscript.contains("会员"),
            estimated_bytes: estimated,
            estimated_duration: duration,
            unavailable_reason: if track.is_some() {
                String::new()
            } else {
                "当前会话未返回此档位，请重新协商或检查会员权限".to_string()
            },
        });
    }
    options
}

/// 优先指定编码，缺失时按 AVC/HEVC/AV1 回退，不改变旧入口默认行为。
pub(crate) fn choose_video_preferred(
    tracks: &[VideoTrack],
    preferred: Option<u32>,
    prefer_codec: Option<u32>,
) -> Option<VideoTrack> {
    if tracks.is_empty() {
        return None;
    }
    let target = preferred
        .filter(|qn| tracks.iter().any(|track| track.qn == *qn))
        .or_else(|| {
            if tracks.iter().any(|track| track.qn == 32) {
                Some(32)
            } else {
                tracks
                    .iter()
                    .filter(|track| track.qn <= 32)
                    .max_by_key(|track| track.qn)
                    .or_else(|| tracks.iter().min_by_key(|track| track.qn))
                    .map(|track| track.qn)
            }
        });
    let qn = target?;
    tracks
        .iter()
        .filter(|track| track.qn == qn)
        .min_by_key(|track| {
            (
                prefer_codec.is_some_and(|codec| codec != track.codec),
                codec_rank(track.codec),
            )
        })
        .cloned()
}

/// 编码优先级：AVC 解码最快，其次 HEVC，最后 AV1。
fn codec_rank(codec: u32) -> u32 {
    match codec {
        7 => 0,
        12 => 1,
        13 => 2,
        _ => 3,
    }
}

/// 查询字幕；缺失与请求错误保持区分，供上层决定是否允许 ASR 回退。
pub(crate) async fn subtitle(
    client: &BiliClient,
    video: &VideoRef,
    cid: u64,
) -> Result<Option<(String, String)>, CommandError> {
    super::subtitle::fetch(client, video, cid).await
}

/// 从 JSON 值中取出字符串数组。
fn strings(value: Option<&Value>) -> Vec<String> {
    value
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(Value::as_str)
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_default()
}

/// 当前 Unix 秒，作为 WBI 时间戳。
pub(crate) fn now_seconds() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}
