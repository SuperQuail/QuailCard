//! 任务内来源共享：短锁发布 future，网络阶段不持锁，不派生脱管任务。
use super::*;
use futures_util::{
    future::{BoxFuture, Shared},
    FutureExt,
};
use std::sync::{Arc, Mutex};

/// 清晰度与编码策略固定在任务内，CID 防止分 P 身份碰撞。
#[derive(Clone, Copy, Hash, Eq, PartialEq)]
pub(super) struct SourceKey {
    pub page: u32,
    pub cid: u64,
    pub quality: Option<u32>,
    pub codec: Option<u32>,
}
type Flight<'a> = Shared<BoxFuture<'a, Result<Option<FrameSource>, CommandError>>>;
struct Slot<'a> {
    attempt: u8,
    flight: Flight<'a>,
    failed: bool,
}

/// 克隆只共享状态，不会为同一分 P 创建第二条下载。
#[derive(Clone, Default)]
pub(super) struct SourceCache<'a>(Arc<Mutex<HashMap<SourceKey, Slot<'a>>>>);
impl<'a> SourceCache<'a> {
    /// 每键最多两次解析；成功永久复用，失败广播给所有等待者再开放一次重试。
    pub async fn get(
        &self,
        key: SourceKey,
        resolve: impl Fn() -> BoxFuture<'a, Result<Option<FrameSource>, CommandError>>,
        cancel: &CancelHandle,
    ) -> Result<Option<FrameSource>, CommandError> {
        loop {
            if cancel() {
                self.0.lock().unwrap_or_else(|e| e.into_inner()).clear();
                return Err(cancelled());
            }
            let (attempt, flight) = {
                let mut entries = self.0.lock().unwrap_or_else(|e| e.into_inner());
                let slot = entries.entry(key).or_insert_with(|| Slot {
                    attempt: 1,
                    flight: resolve().shared(),
                    failed: false,
                });
                if slot.failed && slot.attempt < 2 {
                    slot.attempt += 1;
                    slot.flight = resolve().shared();
                    slot.failed = false;
                }
                (slot.attempt, slot.flight.clone())
            };
            let result = tokio::select! {
                biased;
                _ = observe_cancel(cancel) => {
                    self.0.lock().unwrap_or_else(|e| e.into_inner()).clear();
                    return Err(cancelled());
                },
                result = flight => result,
            };
            if result.is_ok() || result.as_ref().is_err_and(|e| e.code == "VIDEO_CANCELLED") {
                return result;
            }
            {
                let mut entries = self.0.lock().unwrap_or_else(|e| e.into_inner());
                if let Some(slot) = entries.get_mut(&key) {
                    if slot.attempt == attempt {
                        slot.failed = true;
                    }
                }
            }
            if attempt >= 2 {
                return result;
            }
        }
    }
}

/// 来源 future 只捕获解析上下文，避免缓存与自身 future 形成 Arc 环。
#[derive(Clone)]
pub(super) struct SourceResolver<'a> {
    pub deps: FrameDeps<'a>,
    pub storage: VideoStorage,
    pub aid: u64,
    pub quality: Option<u32>,
    pub keys: WbiKeys,
    pub cancel: CancelHandle,
}
impl SourceResolver<'_> {
    /// 仅网络阶段可丢弃 future；下载层自带半文件守卫，不涉及 FFmpeg 子进程。
    pub async fn resolve(&self, window: &PageWindow) -> Result<Option<FrameSource>, CommandError> {
        tokio::select! {
            biased;
            _ = observe_cancel(&self.cancel) => Err(cancelled()),
            result = self.resolve_inner(window) => result,
        }
    }
    /// 已存在的本地轨道直接复用；否则按体积决定整轨下载还是远端按需取帧。
    async fn resolve_inner(
        &self,
        window: &PageWindow,
    ) -> Result<Option<FrameSource>, CommandError> {
        let local = self.storage.media_file(
            self.deps.task_id,
            &format!(
                "video-p{}-cid{}-q{}-c{}.m4s",
                window.page.page,
                window.page.cid,
                self.quality.unwrap_or(0),
                codec_id(&self.deps.settings.prefer_codec).unwrap_or(0)
            ),
        )?;
        if tokio::fs::metadata(&local)
            .await
            .is_ok_and(|meta| meta.len() > 0)
        {
            return Ok(Some(FrameSource::Local(local)));
        }
        let play = media::playurl_quality(
            self.deps.client,
            &self.keys,
            self.aid,
            window.page.cid,
            window.page.duration,
            self.quality,
        )
        .await?;
        let Some(track) = media::choose_video_preferred(
            &play.tracks.videos,
            self.quality,
            codec_id(&self.deps.settings.prefer_codec),
        ) else {
            return Ok(None);
        };
        // 主备地址一起交给抽帧适配器，主 CDN 被拒时仍可尝试官方镜像。
        let mut urls = vec![track.url];
        urls.extend(track.backup);
        if self.deps.stream_only {
            return Ok(Some(FrameSource::RemoteCandidates { urls }));
        }
        let limit = self
            .deps
            .settings
            .video_max_download_mb
            .saturating_mul(1024 * 1024);
        let estimated = (track.bandwidth as f64 * window.duration / 8.0) as u64;
        if estimated <= limit {
            let _permit = match (self.deps.budget, &self.deps.control) {
                (Some(budget), Some(control)) => Some(budget.download(control).await?),
                _ => None,
            };
            if (self.cancel)() {
                return Err(cancelled());
            }
            download::download_first(
                self.deps.client,
                &urls,
                self.deps.referer,
                &local,
                &*self.cancel,
                &mut |_, _| {},
            )
            .await?;
            if tokio::fs::metadata(&local).await?.len() == 0 {
                return Err(CommandError::new("VIDEO_SOURCE_EMPTY", "视频轨道为空"));
            }
            return Ok(Some(FrameSource::Local(local)));
        }
        Ok(Some(FrameSource::RemoteCandidates { urls }))
    }
}

/// 设置编码名映射为 B 站 codec id；未知值交给媒体层自动选择。
pub(super) fn codec_id(codec: &str) -> Option<u32> {
    match codec {
        "avc" => Some(7),
        "hevc" => Some(12),
        "av1" => Some(13),
        _ => None,
    }
}

/// 旧 Agent 只有回调也能取消等待，任务控制块通过合并回调走同一出口。
pub(super) async fn observe_cancel(cancel: &CancelHandle) {
    while !cancel() {
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    }
}
