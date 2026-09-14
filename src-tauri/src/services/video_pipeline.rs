//! 视频任务流水线：元信息 → 字幕 / 转写 → 配图 → 生成笔记 → 落盘。

use std::{collections::HashMap, path::PathBuf};

use crate::{
    error::CommandError,
    services::{
        agent_ports::AgentModel,
        video_note::{self, NoteMeta},
        video_ports::{VideoNotes, VideoTools},
        video_tasks::VideoControl,
    },
    storage::video::{VideoStorage, VideoTaskRecord},
    video::{
        asr,
        bilibili::{download, http::BiliClient, media, wbi::WbiKeys},
        components::{self, Component},
        media::{AsrOptions, CancelHandle, FrameSource},
        models::VideoOutputMode,
        models::VideoSettings,
        transcript::{self, Segment, Transcript},
        url::VideoRef,
    },
};

#[path = "video_pipeline/transcript.rs"]
mod collection;
/// 单帧画面原语：一键流程与 Agent 的 video_shot 工具共用。
#[path = "video_pipeline/frames.rs"]
pub(crate) mod frames;
#[path = "video_pipeline/pages.rs"]
mod pages;
#[path = "video_pipeline/review.rs"]
mod review;
#[path = "video_pipeline/shots.rs"]
mod shots;
use collection::{collect_transcript, resolve_component};
use pages::{locate, PageWindow};
pub(crate) use pages::{page_windows, selected_pages, video_key};
use shots::extract_shots;

/// 流水线依赖：由组合根准备，流水线不自己读取应用状态。
pub(crate) struct PipelineDeps<'a> {
    pub client: BiliClient,
    pub tools: VideoTools<'a>,
    pub notes: &'a dyn VideoNotes,
    pub model: &'a dyn AgentModel,
    pub budget: &'a crate::services::video_budget::VideoBudget,
    pub model_label: String,
    /// 当前供应商是否接受图片输入；false 时不盲配未经核实的画面。
    pub supports_vision: bool,
    pub settings: VideoSettings,
    pub vault_root: PathBuf,
    pub data_dir: PathBuf,
    pub resource_dir: Option<PathBuf>,
    pub manifest_dir: PathBuf,
}

/// 单次任务的输入。
pub(crate) struct PipelineInput {
    pub video: VideoRef,
    pub pages: Vec<u32>,
    pub quality: Option<u32>,
    pub screenshots: bool,
    /// 是否产出文件；false 时只取字，供 Agent 自行成文。
    pub note: bool,
    /// 输出模式：字幕稿保留时间轴，笔记重建信息结构。
    pub mode: VideoOutputMode,
    /// 显式重转写跳过所有转录缓存及平台字幕。
    pub force_transcribe: bool,
}

/// 执行一次完整任务；每个阶段都检查取消。
pub(crate) async fn run(
    deps: PipelineDeps<'_>,
    input: PipelineInput,
    control: VideoControl,
) -> Result<(), CommandError> {
    let storage = VideoStorage::new(&deps.vault_root);
    let task_id = control.snapshot().task_id;
    let result = run_inner(&deps, &input, &control).await;
    let cleanup = storage.cleanup_task_media(&task_id);
    if let Err(error) = &cleanup {
        eprintln!("VIDEO_MEDIA_CLEANUP(detail): {error}");
        control.update(|status| status.message = "临时媒体清理失败，可稍后重试清理".to_string());
    }
    finish_record(&storage, &input, &control, &result)?;
    if let Err(error) = storage.cleanup(deps.settings.keep_media_days) {
        eprintln!("VIDEO_RETENTION_CLEANUP(detail): {error}");
    }
    result
}

/// 失败和取消也必须落盘，保留转录与既有笔记供后续读取。
fn finish_record(
    storage: &VideoStorage,
    input: &PipelineInput,
    control: &VideoControl,
    result: &Result<(), CommandError>,
) -> Result<(), CommandError> {
    let task_id = control.snapshot().task_id;
    if let Err(error) = result {
        let mut record = storage.load_task(&task_id)?.unwrap_or_else(|| {
            VideoTaskRecord::new(&task_id, &input.video.cache_key, &input.video.source_url)
        });
        record.origin = control.origin();
        record.error = Some(error.message.clone());
        touch(
            storage,
            &mut record,
            if error.code == "VIDEO_CANCELLED" {
                "cancelled"
            } else {
                "failed"
            },
            &control.snapshot().step,
            control.snapshot().progress,
        )?;
    }
    Ok(())
}

/// 媒体阶段等待端口回收进程，模型阶段可取消，落盘前再次检查。
async fn run_inner(
    deps: &PipelineDeps<'_>,
    input: &PipelineInput,
    control: &VideoControl,
) -> Result<(), CommandError> {
    if control.is_cancelled() {
        return Err(cancelled());
    }
    let storage = VideoStorage::new(&deps.vault_root);
    let task_id = control.snapshot().task_id;
    let cancel: CancelHandle = {
        let control = control.clone();
        std::sync::Arc::new(move || control.is_cancelled())
    };
    let mut record = storage.load_task(&task_id)?.unwrap_or_else(|| {
        VideoTaskRecord::new(&task_id, &input.video.cache_key, &input.video.source_url)
    });
    // 记录缺失时重建也必须保留来源，否则 Agent 任务会漏进用户历史。
    record.origin = control.origin();

    step(control, "读取视频信息", 8);
    let info = media::video_info(&deps.client, &input.video).await?;
    let selected: Vec<u32> = selected_pages(&info, input)
        .iter()
        .map(|p| p.page)
        .collect();
    if selected.is_empty() || input.pages.iter().any(|p| !selected.contains(p)) {
        return Err(CommandError::validation("所选分 P 不存在"));
    }
    let duration: f64 = selected_pages(&info, input)
        .iter()
        .map(|p| p.duration.max(0.0))
        .sum();
    record.title = info.title.clone();
    record.owner = info.owner.clone();
    record.duration = duration;
    record.quality = input
        .quality
        .or_else(|| deps.settings.video_quality.parse().ok());
    record.pages = selected.clone();
    touch(&storage, &mut record, "running", "读取视频信息", 8)?;
    if cancel() {
        return Err(cancelled());
    }

    step(control, "查找字幕", 25);
    // 分 P 缓存携带模型选择身份；不再复用缺少模型信息的旧任务缓存。
    let transcript = collect_transcript(deps, control, &info, input, &cancel).await?;
    if cancel() {
        return Err(cancelled());
    }
    if transcript::quality(&transcript, duration).insufficient {
        return Err(CommandError::new(
            "VIDEO_TRANSCRIPT_EMPTY",
            "没有识别到足够的语音内容，请确认视频有讲解后再试",
        ));
    }
    storage.save_transcript(&task_id, &transcript)?;
    record.transcript_source = transcript.source.clone();
    touch(&storage, &mut record, "running", "已取得转录", 65)?;
    if cancel() {
        return Err(cancelled());
    }
    control.update(|status| {
        status.transcript_source = transcript.source.clone();
        status.segments = transcript.segments.len() as u32;
    });
    if !input.note {
        touch(&storage, &mut record, "completed", "已取得转录", 100)?;
        return Ok(());
    }

    let meta = NoteMeta {
        title: &info.title,
        owner: &info.owner,
        duration,
        source_url: &input.video.source_url,
        model_label: &deps.model_label,
        transcript_source: &transcript.source,
        max_shots: if input.screenshots && input.mode == VideoOutputMode::Note {
            deps.settings.max_shots
        } else {
            0
        },
    };
    // 字幕模式不调用模型：字幕稿要能逐句核对，改写反而破坏对照关系。
    if input.mode == VideoOutputMode::Transcript {
        return save_transcript(
            deps,
            control,
            &storage,
            &mut record,
            &info,
            &meta,
            &transcript,
        );
    }

    step(control, "生成笔记", 70);
    let progress = |value: u8| {
        control.update(|status| {
            status.progress = value;
            status.segments = transcript.segments.len() as u32;
        });
    };
    let generated = generate_note(deps.model, &transcript, &meta, &progress, control).await?;
    if cancel() {
        return Err(cancelled());
    }

    let mut markdown = generated.markdown;
    let mut shots = 0u32;
    let has_shot_requests = input.screenshots && !generated.shots.is_empty();
    if has_shot_requests {
        step(control, "提取关键画面", 88);
        if !deps.supports_vision {
            // 没有视觉能力时只保留正文，不能将字幕时间点猜测冒充匹配画面。
            control.update(|status| {
                status.message = "当前供应商未启用图片输入，已跳过自动配图并保留正文".to_string()
            });
        }
        match extract_shots(deps, control, &info, input, &markdown, generated.shots).await {
            Ok((updated, count)) => {
                markdown = updated;
                shots = count;
            }
            Err(error) if error.code == "VIDEO_CANCELLED" => return Err(error),
            Err(error) => {
                eprintln!("VIDEO_SHOTS_SKIPPED(detail): {error}");
                control
                    .update(|status| status.message = "截图未能完成，已保留文字笔记".to_string());
            }
        }
    }

    if markdown.contains("[[shot:") {
        control.update(|status| {
            status.message = "部分截图未提取，已移除占位标记并保留文字".to_string()
        });
    }
    markdown = video_note::remove_skipped_shots(&markdown);
    if cancel() {
        return Err(cancelled());
    }
    // 保存阶段只推进步骤，保留配图选中/跳过原因供用户查看。
    control.update(|status| {
        status.step = "保存笔记".to_string();
        status.progress = 96;
        if !has_shot_requests {
            status.message = "保存笔记".to_string();
        }
    });
    let note_path = deps
        .notes
        .write_note(&deps.settings.note_folder, &info.title, &markdown)?;
    record.note_path = Some(note_path.clone());
    touch(&storage, &mut record, "completed", "已完成", 100)?;
    // 任务记录与用户资产不随临时媒体清理一起删除。
    control.update(|status| {
        status.note_path = Some(note_path);
        status.shots = shots;
        status.transcript_source = transcript.source.clone();
        status.segments = transcript.segments.len() as u32;
    });
    Ok(())
}

/// 字幕模式收尾：直接落盘带时间轴的转录稿，不调用模型。
fn save_transcript(
    deps: &PipelineDeps<'_>,
    control: &VideoControl,
    storage: &VideoStorage,
    record: &mut VideoTaskRecord,
    info: &media::VideoInfo,
    meta: &NoteMeta<'_>,
    transcript: &Transcript,
) -> Result<(), CommandError> {
    step(control, "保存字幕稿", 92);
    let markdown = video_note::transcript_markdown(meta, transcript);
    let note_path = deps.notes.write_note(
        &deps.settings.note_folder,
        &format!("{}（字幕稿）", info.title),
        &markdown,
    )?;
    record.note_path = Some(note_path.clone());
    touch(storage, record, "completed", "已完成", 100)?;
    control.update(|status| {
        status.note_path = Some(note_path);
        status.shots = 0;
        status.transcript_source = transcript.source.clone();
        status.segments = transcript.segments.len() as u32;
    });
    Ok(())
}

/// 模型请求可以直接丢弃，取消后不进入截图或笔记落盘阶段。
async fn generate_note(
    model: &dyn AgentModel,
    transcript: &Transcript,
    meta: &NoteMeta<'_>,
    progress: &(dyn Fn(u8) + Send + Sync),
    control: &VideoControl,
) -> Result<video_note::GeneratedNote, CommandError> {
    let log = |message: &str| control.update(|status| status.message = message.to_string());
    tokio::select! {
        biased;
        _ = control.cancelled() => Err(cancelled()),
        result = video_note::generate(model, transcript, meta, progress, &log) => result,
    }
}

#[cfg(test)]
#[path = "video_pipeline/lifecycle_tests.rs"]
mod lifecycle_tests;

/// 更新任务步骤与进度。
fn step(control: &VideoControl, name: &str, progress: u8) {
    control.update(|status| {
        status.step = name.to_string();
        status.message = name.to_string();
        status.progress = progress;
    });
}

/// 持久化任务记录；写入失败必须向调用方传播，不能伪装成功。
fn touch(
    storage: &VideoStorage,
    record: &mut VideoTaskRecord,
    state: &str,
    step_name: &str,
    progress: u8,
) -> Result<(), CommandError> {
    record.state = state.to_string();
    record.step = step_name.to_string();
    record.progress = progress;
    record.updated_at = crate::storage::now_timestamp();
    storage.save_task(record)
}

/// 统一的取消错误。
fn cancelled() -> CommandError {
    CommandError::new("VIDEO_CANCELLED", "已停止视频任务")
}
