//! 无视觉时不允许任何模型、媒体或永久附件调用。
use super::*;
use crate::services::{
    agent_ports::{AgentFuture, AgentModelReply},
    video_tasks::VideoTaskRegistry,
};
use crate::video::media::{AsrEngine, AudioExtractor, FrameExtractor, ProgressHandle, VideoFuture};
use serde_json::Value;
use std::path::Path;

struct Unused;
impl AgentModel for Unused {
    /// 无视觉必须在构造模型请求前返回。
    fn call<'a>(
        &'a self,
        _: &'a str,
        _: &'a [Value],
        _: &'a [crate::ai::ToolDefinition],
        _: &'a (dyn Fn(&str) + Send + Sync),
        _: &'a (dyn Fn(&str) + Send + Sync),
    ) -> AgentFuture<'a, AgentModelReply> {
        panic!("无视觉不应调用模型")
    }
}
impl VideoNotes for Unused {
    /// 配图阶段不得写笔记正文。
    fn write_note(&self, _: &str, _: &str, _: &str) -> Result<String, CommandError> {
        panic!("不应写笔记")
    }
    /// 未经过视觉确认的图片不得落为永久附件。
    fn save_shot(&self, _: &str, _: &str, _: &[u8]) -> Result<String, CommandError> {
        panic!("不应保存附件")
    }
}
impl FrameExtractor for Unused {
    /// 无视觉不需要浪费候选截帧或访问媒体来源。
    fn extract<'a>(
        &'a self,
        _: FrameSource,
        _: f64,
        _: &'a Path,
        _: u32,
        _: CancelHandle,
    ) -> VideoFuture<'a, ()> {
        panic!("不应截帧")
    }
}
impl AudioExtractor for Unused {
    /// 配图不应触发音频处理。
    fn export_wav<'a>(&'a self, _: &'a Path, _: &'a Path, _: CancelHandle) -> VideoFuture<'a, ()> {
        panic!("不应提取音频")
    }
}
impl AsrEngine for Unused {
    /// 配图不应触发语音转录。
    fn transcribe<'a>(
        &'a self,
        _: &'a Path,
        _: &'a AsrOptions,
        _: CancelHandle,
        _: ProgressHandle,
    ) -> VideoFuture<'a, Transcript> {
        panic!("不应转录")
    }
}

#[tokio::test]
/// 真正进入配图入口；组件路径无效仍能清标记保正文并返回明确零配图状态。
async fn no_vision_skips_every_external_port() {
    let record = VideoTaskRecord::new("no-vision", "key", "url");
    let control = VideoTaskRegistry::default()
        .register("owner", &record)
        .unwrap();
    let unused = Unused;
    let deps = PipelineDeps {
        client: BiliClient::new(crate::video::bilibili::http::CookieJar::default()).unwrap(),
        tools: VideoTools {
            audio: &unused,
            frames: &unused,
            asr: &unused,
        },
        notes: &unused,
        model: &unused,
        budget: &crate::services::video_budget::VideoBudget::default(),
        model_label: "test".into(),
        supports_vision: false,
        settings: VideoSettings {
            ffmpeg_path: "missing-test-ffmpeg/ffmpeg.exe".into(),
            ..Default::default()
        },
        vault_root: PathBuf::new(),
        data_dir: PathBuf::new(),
        resource_dir: None,
        manifest_dir: PathBuf::new(),
    };
    let crate::video::url::VideoInput::Video(video) = crate::video::url::parse("av42").unwrap()
    else {
        panic!("视频输入")
    };
    let input = PipelineInput {
        video,
        pages: vec![],
        quality: None,
        screenshots: true,
        note: true,
        mode: crate::video::models::VideoOutputMode::Note,
        force_transcribe: false,
    };
    let info = media::VideoInfo {
        aid: 42,
        bvid: "".into(),
        title: "标题".into(),
        owner: "作者".into(),
        duration: 120.0,
        pages: vec![],
    };
    let shots = vec![video_note::ShotRequest {
        seconds: 60.0,
        marker: "[[shot:01:00]]".into(),
        target: "函数定义".into(),
    }];
    let (text, count) = extract_shots(
        &deps,
        &control,
        &info,
        &input,
        "正文[[shot:01:00]]后文",
        shots,
    )
    .await
    .unwrap();
    assert_eq!(count, 0);
    assert!(text.starts_with("正文后文"));
    assert!(!text.contains("[[shot:"));
    let status = control.snapshot();
    assert!(status.message.contains("已选择 0 张"));
    assert!(status.message.contains("跳过 1 个目标"));
    assert!(status.message.contains("未启用视觉"));
}
