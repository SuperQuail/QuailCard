//! 单帧画面原语：共享来源、受预算约束的抽帧与顺序附件提交。
use super::*;
use crate::services::video_budget::VideoBudget;
use futures_util::FutureExt;
use std::sync::Arc;

#[cfg(test)]
#[path = "frames/source_tests.rs"]
mod source_tests;

#[path = "frames/selected.rs"]
mod selection;
#[path = "frames/sources.rs"]
mod sources;
pub(crate) use selection::SelectedFrame;
use sources::{codec_id, observe_cancel, SourceCache, SourceKey, SourceResolver};

/// 依赖只借用组合根资源；旧 Agent 可不提供预算和任务控制块。
#[derive(Clone)]
pub(crate) struct FrameDeps<'a> {
    pub client: &'a BiliClient,
    pub frames: &'a dyn crate::video::media::FrameExtractor,
    pub notes: &'a dyn VideoNotes,
    pub settings: &'a VideoSettings,
    pub vault_root: &'a std::path::Path,
    pub task_id: &'a str,
    pub referer: &'a str,
    pub stream_only: bool,
    pub budget: Option<&'a VideoBudget>,
    pub control: Option<VideoControl>,
}

/// 已保存的一帧保留原字节，兼容 Agent 和旧串行调用者。
pub(crate) struct GrabbedFrame {
    pub seconds: f64,
    pub stamp: String,
    pub markdown_path: String,
    pub bytes: Vec<u8>,
}

/// 模型实际查看的候选原字节；临时路径由 VideoStorage 净化产生。
pub(crate) struct CandidateFrame {
    pub seconds: f64,
    pub stamp: String,
    pub(super) file_name: String,
    pub path: PathBuf,
    pub bytes: Vec<u8>,
}

/// 克隆共享任务内 single-flight，工作项自身不持有独立来源缓存。
#[derive(Clone)]
pub(crate) struct FrameGrabber<'a> {
    deps: FrameDeps<'a>,
    storage: VideoStorage,
    windows: Vec<PageWindow>,
    resolver: SourceResolver<'a>,
    sources: SourceCache<'a>,
    cancel: CancelHandle,
}

impl<'a> FrameGrabber<'a> {
    /// WBI 仅预备一次，两个取消来源合并后贯穿全部阶段。
    pub(crate) async fn start(
        deps: FrameDeps<'a>,
        info: &media::VideoInfo,
        input: &PipelineInput,
        cancel: CancelHandle,
    ) -> Result<Self, CommandError> {
        let control = deps.control.clone();
        let cancel: CancelHandle =
            Arc::new(move || cancel() || control.as_ref().is_some_and(VideoControl::is_cancelled));
        let keys = tokio::select! {
            biased;
            _ = observe_cancel(&cancel) => return Err(cancelled()),
            result = media::wbi_keys(deps.client) => result?,
        };
        let storage = VideoStorage::new(deps.vault_root);
        let resolver = SourceResolver {
            deps: deps.clone(),
            storage: storage.clone(),
            aid: info.aid,
            quality: input
                .quality
                .or_else(|| deps.settings.video_quality.parse().ok()),
            keys,
            cancel: cancel.clone(),
        };
        Ok(Self {
            deps,
            storage,
            windows: page_windows(info, input),
            resolver,
            sources: SourceCache::default(),
            cancel,
        })
    }

    /// 搜索窗口固定在锚点所属分 P，不能越界借用相邻内容。
    pub(crate) fn search_bounds(&self, at: f64) -> Option<(f64, f64)> {
        let (index, _) = locate(&self.windows, at)?;
        let window = &self.windows[index];
        Some((window.start, window.start + window.duration))
    }

    /// 旧立即保存接口仍可用，但并发配图应先收集 SelectedFrame 再顺序提交。
    pub(crate) async fn grab(&self, at: f64) -> Result<Option<GrabbedFrame>, CommandError> {
        let Some(candidate) = self.grab_candidate(at).await? else {
            return Ok(None);
        };
        self.persist(candidate).map(Some)
    }

    /// 默认单图上限为 12 MiB；组预算由调用者通过 limited 版本收紧。
    pub(crate) async fn grab_candidate(
        &self,
        at: f64,
    ) -> Result<Option<CandidateFrame>, CommandError> {
        self.grab_candidate_limited(at, selection::MAX_IMAGE_BYTES)
            .await
    }

    /// 先共享来源、再取得抽帧额度；直接等待媒体端口回收真实进程，禁止 select 丢弃它。
    pub(crate) async fn grab_candidate_limited(
        &self,
        at: f64,
        max_bytes: usize,
    ) -> Result<Option<CandidateFrame>, CommandError> {
        if (self.cancel)() {
            return Err(cancelled());
        }
        let Some((index, local_seconds)) = locate(&self.windows, at) else {
            return Ok(None);
        };
        let window = self.windows[index].clone();
        let key = SourceKey {
            page: window.page.page,
            cid: window.page.cid,
            quality: self.resolver.quality,
            codec: codec_id(&self.deps.settings.prefer_codec),
        };
        let source = self
            .sources
            .get(
                key,
                || {
                    let resolver = self.resolver.clone();
                    let window = window.clone();
                    async move { resolver.resolve(&window).await }.boxed()
                },
                &self.cancel,
            )
            .await?;
        let Some(source) = source else {
            return Ok(None);
        };
        let file_name = format!(
            "shot-p{}-{}-{}",
            window.page.page,
            (at * 1000.0).round() as u64,
            uuid::Uuid::now_v7()
        );
        let target = self
            .storage
            .shot_file(self.deps.task_id, &format!("{file_name}.jpg"))?;
        let mut cleanup = selection::PendingFile(Some(target.clone()));
        {
            let _permit = match (self.deps.budget, &self.deps.control) {
                (Some(budget), Some(control)) => Some(budget.frame(control).await?),
                _ => None,
            };
            if (self.cancel)() {
                return Err(cancelled());
            }
            self.deps
                .frames
                .extract(
                    source,
                    local_seconds,
                    &target,
                    self.deps.settings.shot_max_width,
                    self.cancel.clone(),
                )
                .await?;
        }
        if (self.cancel)() {
            return Err(cancelled());
        }
        // 有界同步读取不会留下取消后仍运行的后台文件读取任务。
        let bytes = selection::read_limited(&target, max_bytes)?;
        if (self.cancel)() {
            return Err(cancelled());
        }
        cleanup.0 = None;
        Ok(Some(CandidateFrame {
            seconds: at,
            stamp: transcript::format_timestamp(at),
            file_name,
            path: target,
            bytes,
        }))
    }

    /// 旧提交仍保存调用者已有字节，不要求 mock 候选具备临时文件。
    pub(crate) fn persist(&self, frame: CandidateFrame) -> Result<GrabbedFrame, CommandError> {
        if (self.cancel)() {
            return Err(cancelled());
        }
        let markdown_path = self.deps.notes.save_shot(
            &self.deps.settings.note_folder,
            &frame.file_name,
            &frame.bytes,
        )?;
        Ok(GrabbedFrame {
            seconds: frame.seconds,
            stamp: frame.stamp,
            markdown_path,
            bytes: frame.bytes,
        })
    }

    /// 释放已送审的大字节，仅携带同一临时文件与 SHA-256 身份进入结果队列。
    pub(crate) fn selected(candidate: CandidateFrame) -> SelectedFrame {
        selection::selected(candidate)
    }

    /// 汇聚者顺序调用；先重新净化路径、核实原内容，再保存为永久附件。
    pub(crate) fn persist_selected(
        &self,
        selected: SelectedFrame,
    ) -> Result<GrabbedFrame, CommandError> {
        if (self.cancel)() {
            return Err(cancelled());
        }
        let expected = self
            .storage
            .shot_file(self.deps.task_id, &format!("{}.jpg", selected.file_name))?;
        let bytes = selection::verify(&selected, &expected)?;
        self.persist(CandidateFrame {
            seconds: selected.seconds,
            stamp: selected.stamp,
            file_name: selected.file_name,
            path: selected.path,
            bytes,
        })
    }
}
