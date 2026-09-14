//! 视频转笔记的对外类型（与前端 src/domain/video.ts 同步）。

use serde::{Deserialize, Serialize};

/// 分 P 摘要。
#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct VideoPage {
    pub page: u32,
    pub title: String,
    pub duration: f64,
}

/// 清晰度选项。
#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct VideoQuality {
    pub qn: u32,
    pub label: String,
    pub height: u32,
    pub available: bool,
    pub requires_vip: bool,
    pub estimated_bytes: u64,
    pub estimated_duration: f64,
    pub unavailable_reason: String,
}

/// 链接解析结果，先展示再开始任务。
#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct VideoProbe {
    pub bvid: String,
    pub title: String,
    pub owner: String,
    pub duration: f64,
    pub pages: Vec<VideoPage>,
    pub qualities: Vec<VideoQuality>,
    pub selected_page: u32,
    pub logged_in: bool,
}

/// 任务状态快照；序号用于丢弃迟到响应。
#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VideoTaskStatus {
    pub task_id: String,
    pub state: String,
    pub step: String,
    pub progress: u8,
    pub message: String,
    pub sequence: u64,
    pub segments: u32,
    pub shots: u32,
    pub transcript_source: String,
    pub backend: String,
    /// 当前分 P、当前尝试的真实 Whisper 进度；不等同于流水线加权进度。
    pub asr_progress: Option<AsrProgress>,
    pub logs: Vec<String>,
    pub note_path: Option<String>,
    pub error: Option<String>,
}

/// Whisper 回调观测值；音频总时长来自视频元数据，不能作为精确转录时间戳。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AsrProgress {
    pub page: u32,
    pub attempt: u32,
    pub percent: Option<u8>,
    pub total_audio_seconds: Option<f64>,
    pub elapsed_seconds: f64,
}

/// 扫码登录状态。
#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VideoLoginStatus {
    pub state: String,
    pub message: String,
    pub image: String,
    pub logged_in: bool,
    /// 已登录时的 B 站昵称；未登录或接口未返回时为空。
    pub name: String,
    /// 已登录时的头像地址；前端只通过后端代理读取，不直连第三方。
    pub avatar: String,
}

/// 外部组件状态。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VideoComponentStatus {
    /// 稳定标识（ffmpeg / whisper）；展示名可改，界面关联关系不随文案漂移。
    pub id: String,
    pub name: String,
    pub available: bool,
    pub path: String,
    pub source: String,
    pub detail: String,
}

/// 语音模型状态。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VideoModelStatus {
    pub id: String,
    pub label: String,
    pub bytes: u64,
    pub status: String,
}

/// 视频设置小节；缺失字段取默认值，写入时写全量。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct VideoSettings {
    /// 笔记落盘目录（Vault 内相对路径）。
    pub note_folder: String,
    /// 语音模型档位 id。
    pub asr_model: String,
    /// 无字幕时是否允许本地转写。
    pub asr_enabled: bool,
    /// 是否生成关键帧截图。
    pub screenshots_enabled: bool,
    /// 单篇截图数量上限。
    pub max_shots: u32,
    /// 默认清晰度：auto 或 qn 字符串。
    pub video_quality: String,
    /// 编码偏好：avc / hevc / av1。
    pub prefer_codec: String,
    /// 超过该体积改用远端定位取帧。
    pub video_max_download_mb: u64,
    /// 截图最长边像素。
    pub shot_max_width: u32,
    /// 用户指定的 ffmpeg 路径。
    pub ffmpeg_path: String,
    /// 用户指定的 whisper-cli 路径。
    pub whisper_path: String,
    /// 模型与加速包下载镜像地址。
    pub model_mirror: String,
    /// 临时媒体保留天数。
    pub keep_media_days: u64,
}

impl Default for VideoSettings {
    /// 面向中文学习场景的默认值：small 模型、480P、12 张截图、保留 7 天。
    fn default() -> Self {
        Self {
            note_folder: "视频笔记".to_string(),
            asr_model: "small".to_string(),
            asr_enabled: true,
            screenshots_enabled: true,
            max_shots: 12,
            video_quality: "auto".to_string(),
            prefer_codec: "avc".to_string(),
            video_max_download_mb: 300,
            shot_max_width: 1600,
            ffmpeg_path: String::new(),
            whisper_path: String::new(),
            model_mirror: String::new(),
            keep_media_days: 7,
        }
    }
}

impl VideoSettings {
    /// 配置写入前验证路径与资源上限，失败不会改变内存或磁盘设置。
    pub(crate) fn validate(&self) -> Result<(), crate::error::CommandError> {
        use crate::error::CommandError;
        if !self.note_folder.is_empty() {
            crate::vaultfs::sanitize_relative(&self.note_folder)?;
        }
        if super::asr::find(&self.asr_model).is_none() {
            return Err(CommandError::validation("未知的语音模型"));
        }
        if self.video_quality != "auto"
            && !self
                .video_quality
                .parse::<u32>()
                .is_ok_and(|value| value > 0 && value <= 1000)
        {
            return Err(CommandError::validation("清晰度必须为自动或有效档位编号"));
        }
        if !matches!(self.prefer_codec.as_str(), "auto" | "avc" | "hevc" | "av1") {
            return Err(CommandError::validation("不支持的视频编码偏好"));
        }
        if self.max_shots > 100
            || !(64..=4096).contains(&self.shot_max_width)
            || self.video_max_download_mb == 0
            || self.video_max_download_mb > 100_000
            || self.keep_media_days > 3650
        {
            return Err(CommandError::validation(
                "截图、下载体积或保留天数超出支持范围",
            ));
        }
        if !self.model_mirror.trim().is_empty() {
            let url = reqwest::Url::parse(self.model_mirror.trim())
                .map_err(|_| CommandError::validation("模型镜像地址无效"))?;
            if url.scheme() != "https"
                || url.host_str().is_none()
                || !url.username().is_empty()
                || url.password().is_some()
                || url.query().is_some()
                || url.fragment().is_some()
            {
                return Err(CommandError::validation(
                    "模型镜像必须是不含凭据、查询参数的 HTTPS 地址",
                ));
            }
        }
        Ok(())
    }
}

/// 视频输出模式：字幕保留时间轴，笔记提取并重建信息、不写时间轴。
///
/// 只增不改：新增取值时旧客户端缺省仍按笔记模式处理。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub enum VideoOutputMode {
    /// 笔记模式（默认）：结构化笔记，可配关键画面。
    #[default]
    Note,
    /// 字幕模式：保留时间轴的转录稿，不生成笔记、不配图。
    Transcript,
}

/// 开始任务的输入。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VideoStartInput {
    pub url: String,
    pub provider_id: String,
    /// 期望清晰度 qn；None 表示使用设置中的默认值。
    #[serde(default)]
    pub quality: Option<u32>,
    /// 选中的分 P；空表示仅处理 P1。
    #[serde(default)]
    pub pages: Vec<u32>,
    /// 本次是否生成截图；None 表示使用设置值。
    #[serde(default)]
    pub screenshots: Option<bool>,
    /// 显式跳过缓存与平台字幕，使用当前模型重新转写。
    #[serde(default)]
    pub force_transcribe: bool,
    /// 输出模式；缺省为笔记模式，与旧客户端行为一致。
    #[serde(default)]
    pub mode: VideoOutputMode,
}

/// 模型下载进度快照；与下载执行解耦，关闭面板也可再次查询。
#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VideoDownloadStatus {
    pub model_id: String,
    pub state: String,
    pub downloaded_bytes: u64,
    pub total_bytes: Option<u64>,
    pub bytes_per_second: u64,
    pub error: Option<String>,
}

/// 可恢复参数的视频任务摘要，历史记录不会自动重启任务。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VideoTaskHistory {
    pub task_id: String,
    pub url: String,
    pub title: String,
    pub state: String,
    pub updated_at: i64,
    pub note_path: Option<String>,
    pub error: Option<String>,
    pub pages: Vec<u32>,
    pub quality: Option<u32>,
}
