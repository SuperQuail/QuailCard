//! 学习 Agent 视频适配器：可选择执行参数、分页读取完整转录、按时间点抽画面。
use crate::{
    error::CommandError,
    services::{
        agent_ports::{AgentFuture, AgentVideo},
        video_pipeline::{
            frames::{FrameDeps, FrameGrabber},
            PipelineInput,
        },
        AppServices,
    },
    storage::{video::VideoStorage, Storage},
    vaultfs::VaultState,
    video::{
        bilibili::{http::BiliClient, media},
        components::Component,
        media::{ffmpeg::Ffmpeg, CancelHandle},
        models::{VideoOutputMode, VideoStartInput},
        transcript::Transcript,
    },
    video_bridge,
};
#[path = "agent_video_progress.rs"]
mod progress;

use base64::Engine as _;
use serde_json::{json, Value};
use tauri::Manager;

/// 工具调用固定供应商和归属会话，避免跨窗口访问任务。
pub(crate) struct VideoAdapter {
    pub(crate) app: tauri::AppHandle,
    pub(crate) provider_id: String,
    pub(crate) owner: String,
}

impl AgentVideo for VideoAdapter {
    /// 归属与运行态由注册表筛选；原始日志和字幕正文不进入阶段事件。
    fn progress(&self) -> Option<String> {
        self.app
            .state::<AppServices>()
            .video_tasks
            .running_status(&self.owner)
            .map(|status| progress::summary(&status))
    }

    /// 兼容旧调用方；未指定选项时尊重当前设置。
    fn run<'a>(&'a self, url: &'a str, note: bool) -> AgentFuture<'a, Value> {
        Box::pin(async move { self.run_options(url, note, &json!({})).await })
    }

    /// 运行用户所选分 P、清晰度和截图参数，转录仅首屏进入模型上下文。
    fn run_options<'a>(
        &'a self,
        url: &'a str,
        note: bool,
        options: &'a Value,
    ) -> AgentFuture<'a, Value> {
        Box::pin(async move {
            let input = VideoStartInput {
                url: url.to_string(),
                provider_id: self.provider_id.clone(),
                quality: options["quality"].as_u64().map(|n| n as u32),
                pages: options["pages"]
                    .as_array()
                    .map(|v| {
                        v.iter()
                            .filter_map(Value::as_u64)
                            .map(|n| n as u32)
                            .collect()
                    })
                    .unwrap_or_default(),
                screenshots: options["screenshots"].as_bool(),
                force_transcribe: options["forceTranscribe"].as_bool().unwrap_or(false),
                // 取字工具走字幕模式，成文工具走笔记模式。
                mode: if note {
                    VideoOutputMode::Note
                } else {
                    VideoOutputMode::Transcript
                },
            };
            let status = video_bridge::run_now(&self.app, &self.owner, input, note).await?;
            let mut summary = json!({
                "taskId": status.task_id, "state": status.state,
                "output": if note { "note" } else { "transcript" },
                "notePath": status.note_path, "segments": status.segments,
                "shots": status.shots, "transcriptSource": status.transcript_source,
            });
            if !note {
                summary["transcript"] = self.read_transcript(&status.task_id, 0, 6000).await?;
                summary["readMoreTool"] = json!("video_transcript_read");
            }
            Ok(summary)
        })
    }

    /// 读取前验证任务归属，字符偏移可覆盖全文且不拆开 UTF-8 字节。
    fn read_transcript<'a>(
        &'a self,
        task_id: &'a str,
        offset: usize,
        limit: usize,
    ) -> AgentFuture<'a, Value> {
        Box::pin(async move {
            self.app
                .state::<AppServices>()
                .video_tasks
                .status(&self.owner, task_id)?;
            let root = video_bridge::vault_root(&self.app)?;
            let transcript = VideoStorage::new(&root)
                .load_transcript(task_id)?
                .ok_or_else(|| CommandError::new("VIDEO_NO_TRANSCRIPT", "任务转录不存在"))?;
            Ok(transcript_page(&transcript, offset, limit))
        })
    }

    /// 按时间点抽一帧交给模型查看；模型据此决定这张图能不能用。
    ///
    /// 只读任务与视频，不写笔记正文；图片走按需远端取帧，不整轨下载。
    fn shot<'a>(&'a self, task_id: &'a str, at: f64) -> AgentFuture<'a, Value> {
        Box::pin(async move {
            if !at.is_finite() || !(0.0..=86_400.0).contains(&at) {
                return Err(CommandError::validation("截图时间点无效"));
            }
            self.app
                .state::<AppServices>()
                .video_tasks
                .status(&self.owner, task_id)?;
            let storage = self.app.state::<Storage>();
            let config = storage
                .get_provider_config(&self.provider_id)
                .await?
                .ok_or_else(|| CommandError::validation("请先在设置中选择模型供应商"))?;
            if !config.supports_vision {
                return Err(CommandError::validation(
                    "当前供应商未启用图片输入，无法查看截图；请在模型设置中开启或改用支持视觉的模型",
                ));
            }
            let settings = storage.get_video_settings().await;
            let root = video_bridge::vault_root(&self.app)?;
            let record = VideoStorage::new(&root)
                .load_task(task_id)?
                .ok_or_else(|| CommandError::new("VIDEO_TASK_MISSING", "视频任务不存在"))?;
            let ffmpeg_path = video_bridge::component_path(&self.app, &settings, Component::Ffmpeg);
            if !ffmpeg_path.is_file() {
                return Err(CommandError::new(
                    "VIDEO_COMPONENT_MISSING",
                    "缺少媒体组件（ffmpeg），请重新安装或指定其路径",
                ));
            }
            let client = BiliClient::new(video_bridge::cookie(&self.app).await?)?;
            let video = video_bridge::resolve_video(&client, &record.source_url).await?;
            let info = media::video_info(&client, &video).await?;
            let input = PipelineInput {
                video,
                pages: record.pages.clone(),
                quality: record.quality,
                screenshots: true,
                note: false,
                mode: VideoOutputMode::Note,
                force_transcribe: false,
            };
            let ffmpeg = Ffmpeg::new(ffmpeg_path);
            // 截图要落进笔记附件目录，所以这里和任务执行时一样绑定同一个知识库根目录。
            let vault = VaultState::new();
            vault.set_root(root.clone())?;
            let notes = video_bridge::VaultNotes { vault };
            let cancel: CancelHandle = std::sync::Arc::new(|| false);
            let deps = FrameDeps {
                budget: None,
                control: None,
                client: &client,
                frames: &ffmpeg,
                notes: &notes,
                settings: &settings,
                vault_root: &root,
                task_id,
                referer: &input.video.page_url(),
                stream_only: true,
            };
            let grabber = FrameGrabber::start(deps, &info, &input, cancel).await?;
            let frame = grabber
                .grab(at)
                .await?
                .ok_or_else(|| CommandError::validation("这个时间点超出视频范围，请换一个秒数"))?;
            let encoded = base64::engine::general_purpose::STANDARD.encode(&frame.bytes);
            Ok(json!({
                "path": frame.markdown_path,
                "at": frame.seconds,
                "stamp": frame.stamp,
                "image": {"mimeType": "image/jpeg", "dataBase64": encoded}
            }))
        })
    }
}

/// 页长硬限制为 12000 字符，返回游标让模型按需读取而非一次灌入全文。
fn transcript_page(transcript: &Transcript, offset: usize, limit: usize) -> Value {
    let limit = limit.clamp(1, 12000);
    let mut total = 0usize;
    let mut text = String::new();
    let end = offset.saturating_add(limit);
    for segment in &transcript.segments {
        let line = format!(
            "[{}] {}\n",
            crate::video::transcript::format_timestamp(segment.start),
            segment.text
        );
        for character in line.chars() {
            if total >= offset && total < end {
                text.push(character);
            }
            total = total.saturating_add(1);
        }
    }
    json!({"text": text, "offset": offset, "totalCharacters": total,
        "nextOffset": if end < total { Some(end) } else { None }})
}

#[cfg(test)]
mod tests {
    use super::*;
    /// 超长单片段也严格分页，连续游标可还原完整中文原文。
    #[test]
    fn transcript_pages_cover_long_unicode_segment() {
        let transcript = Transcript {
            language: "zh".into(),
            source: "whisper".into(),
            segments: vec![crate::video::transcript::Segment {
                start: 0.0,
                end: 1.0,
                text: "字".repeat(16000),
            }],
        };
        let first = transcript_page(&transcript, 0, usize::MAX);
        assert_eq!(first["text"].as_str().unwrap().chars().count(), 12000);
        assert_eq!(first["nextOffset"], 12000);
        let second = transcript_page(&transcript, 12000, 12000);
        assert!(second["nextOffset"].is_null());
        assert_eq!(
            first["text"].as_str().unwrap().chars().count()
                + second["text"].as_str().unwrap().chars().count(),
            first["totalCharacters"].as_u64().unwrap() as usize
        );
    }
}
