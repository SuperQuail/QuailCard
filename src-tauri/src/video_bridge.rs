//! 视频任务的组合根：把存储、组件、模型与笔记端口接起来。
//!
//! 命令层与学习 Agent 都从这里启动任务，避免两处重复装配。

#[path = "video_credentials.rs"]
pub(crate) mod credentials;
#[path = "video_execution.rs"]
mod execution;

use std::path::PathBuf;

use tauri::Manager;

use crate::{
    agent_bridge,
    error::CommandError,
    services::{
        video_pipeline::{self, PipelineDeps, PipelineInput},
        video_ports::{VideoNotes, VideoTools},
        video_tasks::VideoControl,
        AppServices,
    },
    storage::{
        video::{TaskOrigin, VideoTaskRecord},
        Storage,
    },
    vaultfs::VaultState,
    video::{
        asr::whisper_cli::WhisperCli,
        bilibili::{
            http::{BiliClient, CookieJar},
            media,
        },
        components::{self, Component},
        media::ffmpeg::Ffmpeg,
        models::{VideoSettings, VideoStartInput, VideoTaskStatus},
        url::{self, VideoInput, VideoRef},
    },
};

/// 保险库中 B 站 Cookie 的引用名。
pub(crate) const BILI_CREDENTIAL: &str = "bilibili_cookie";

/// 笔记写入适配器：把 Vault 能力包成端口。
pub(crate) struct VaultNotes {
    pub(crate) vault: VaultState,
}

impl VideoNotes for VaultNotes {
    /// 新建笔记后写入正文；同名由 Vault 层自动加序号。
    fn write_note(&self, folder: &str, title: &str, content: &str) -> Result<String, CommandError> {
        let path = self.vault.create_note(folder, title)?;
        self.vault.write_note(&path, content)?;
        Ok(path)
    }

    /// 配图写入附件目录，返回相对笔记目录的路径。
    fn save_shot(
        &self,
        note_folder: &str,
        file_name: &str,
        bytes: &[u8],
    ) -> Result<String, CommandError> {
        self.vault
            .save_generated_image(note_folder, file_name, bytes)
    }
}

/// 读取当前知识库根目录；未打开时给出可操作提示。
pub(crate) fn vault_root(app: &tauri::AppHandle) -> Result<PathBuf, CommandError> {
    app.state::<VaultState>()
        .root()?
        .ok_or_else(|| CommandError::new("VAULT_NOT_OPEN", "请先打开知识库"))
}

/// 应用数据目录，用于模型与加速组件。
pub(crate) fn data_dir(app: &tauri::AppHandle) -> Result<PathBuf, CommandError> {
    app.path()
        .app_data_dir()
        .map_err(|_| CommandError::new("FILE_ERROR", "无法定位应用数据目录"))
}

/// 资源目录，发布后随包组件所在位置。
pub(crate) fn resource_dir(app: &tauri::AppHandle) -> Option<PathBuf> {
    app.path().resource_dir().ok()
}

/// 源码资源目录，开发期回退使用。
pub(crate) fn manifest_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

/// 读取保险库中的 B 站 Cookie；仅凭据确实缺失时允许游客模式。
pub(crate) async fn cookie(app: &tauri::AppHandle) -> Result<CookieJar, CommandError> {
    let storage = app.state::<Storage>();
    let services = app.state::<AppServices>();
    credentials::load(
        services
            .vault
            .get_credential(&storage, BILI_CREDENTIAL)
            .await,
    )
}

/// 构造客户端前读取凭据，返回前确认非空凭据仍被主站认可。
pub(crate) async fn client(app: &tauri::AppHandle) -> Result<BiliClient, CommandError> {
    let client = BiliClient::new(cookie(app).await?)?;
    credentials::validate(&client).await?;
    Ok(client)
}

/// 解析用户输入；b23.tv 短链在这里跟随跳转。
pub(crate) async fn resolve_video(
    client: &BiliClient,
    raw: &str,
) -> Result<VideoRef, CommandError> {
    match url::parse(raw)? {
        VideoInput::Video(video) => Ok(video),
        VideoInput::ShortLink(link) => {
            let resolved = media::resolve_short_link(client, &link).await?;
            match url::parse_resolved(&resolved)? {
                VideoInput::Video(video) => Ok(video),
                VideoInput::ShortLink(_) => {
                    Err(CommandError::new("VIDEO_URL_INVALID", "无法解析该短链"))
                }
            }
        }
    }
}

/// 解析组件路径；找不到时返回空路径，流水线会在真正需要时给出明确提示。
pub(crate) fn component_path(
    app: &tauri::AppHandle,
    settings: &VideoSettings,
    component: Component,
) -> PathBuf {
    let override_path = match component {
        Component::Ffmpeg => &settings.ffmpeg_path,
        Component::Whisper => &settings.whisper_path,
    };
    let data = data_dir(app).ok();
    let resources = resource_dir(app);
    let manifest = manifest_dir();
    components::resolve(
        component,
        Some(override_path),
        resources.as_deref(),
        data.as_deref(),
        Some(&manifest),
    )
    .map(|resolved| resolved.path)
    .unwrap_or_default()
}

/// 已登记的任务装配结果；所有字段拥有所有权，便于移入异步任务。
pub(crate) struct Prepared {
    execution: execution::Execution,
    pub(crate) control: VideoControl,
    pub(crate) client: BiliClient,
    pub(crate) video: VideoRef,
    pub(crate) settings: VideoSettings,
    pub(crate) vault_root: PathBuf,
    pub(crate) data_dir: PathBuf,
    pub(crate) resource_dir: Option<PathBuf>,
    pub(crate) manifest_dir: PathBuf,
    pub(crate) model: crate::ai::agent::ConfiguredAgentModel,
    pub(crate) model_label: String,
    pub(crate) budget: crate::services::video_budget::VideoBudget,
    /// 供应商图片能力：决定截图是否交给模型复查。
    pub(crate) supports_vision: bool,
    pub(crate) ffmpeg_path: PathBuf,
    pub(crate) whisper_path: PathBuf,
    pub(crate) vault: VaultState,
    pub(crate) input: VideoStartInput,
    /// 是否生成笔记；false 时只取字，供 Agent 自行成文。
    pub(crate) note: bool,
}

/// 登记任务并装配全部依赖；失败时任务状态已经是终态。
///
/// 来源随记录落盘：Agent 任务不会出现在用户历史里。
pub(crate) async fn prepare(
    app: &tauri::AppHandle,
    owner: &str,
    input: VideoStartInput,
    note: bool,
    origin: TaskOrigin,
) -> Result<Prepared, CommandError> {
    let storage = app.state::<Storage>();
    let settings = storage.get_video_settings().await;
    let config = storage
        .get_provider_config(&input.provider_id)
        .await?
        .ok_or_else(|| CommandError::validation("请先在设置中选择模型供应商"))?;
    let client = client(app).await?;
    let video = resolve_video(&client, &input.url).await?;
    let root = vault_root(app)?;
    // 新任务开始前清掉已经无法再被工具或界面访问的 Agent 记录，避免批量任务把历史与磁盘撑大。
    crate::video_history::purge_finished(app);
    let task_id = uuid::Uuid::now_v7().to_string();
    let mut record = VideoTaskRecord::new(&task_id, &video.cache_key, &video.source_url);
    record.state = "running".to_string();
    record.origin = origin;
    let (model, _learning) =
        agent_bridge::model(app, &config.id, &format!("video-{task_id}")).await?;
    let model_label = format!("{} · {}", config.protocol, config.model);
    // 根目录在准备开始时固定，模型装配期间切库不能使任务写入另一个知识库。
    let vault = VaultState::new();
    vault.set_root(root.clone())?;
    let data = data_dir(app)?;
    let resources = resource_dir(app);
    let manifest = manifest_dir();
    let ffmpeg_path = component_path(app, &settings, Component::Ffmpeg);
    let whisper_path = component_path(app, &settings, Component::Whisper);
    // 所有可失败装配完成后才占用窗口任务槽；登记后持久化失败立即终结。
    let control =
        app.state::<AppServices>()
            .video_tasks
            .register_persisted(owner, &record, || {
                crate::storage::video::VideoStorage::new(&root).save_task(&record)
            })?;
    let execution = execution::Execution::new(
        control.clone(),
        crate::storage::video::VideoStorage::new(&root),
    );
    Ok(Prepared {
        execution,
        control,
        client,
        video,
        settings,
        vault_root: root,
        data_dir: data,
        resource_dir: resources,
        manifest_dir: manifest,
        model,
        model_label,
        supports_vision: config.supports_vision,
        budget: app.state::<AppServices>().video_budget.clone(),
        ffmpeg_path,
        whisper_path,
        vault,
        input,
        note,
    })
}

/// 运行装配好的任务；返回最终快照。
pub(crate) async fn execute(prepared: Prepared) -> Result<VideoTaskStatus, CommandError> {
    let Prepared {
        mut execution,
        budget,
        control,
        client,
        video,
        settings,
        vault_root,
        data_dir,
        resource_dir,
        manifest_dir,
        model,
        model_label,
        supports_vision,
        ffmpeg_path,
        whisper_path,
        vault,
        input,
        note,
    } = prepared;
    let ffmpeg = Ffmpeg::new(ffmpeg_path);
    let whisper = WhisperCli::new(whisper_path);
    let notes = VaultNotes { vault };
    let tools = VideoTools {
        audio: &ffmpeg,
        frames: &ffmpeg,
        asr: &whisper,
    };
    let pipeline_input = PipelineInput {
        video,
        pages: input.pages.clone(),
        quality: input.quality,
        // 字幕模式不配图：截图只在笔记模式有意义。
        screenshots: input.screenshots.unwrap_or(settings.screenshots_enabled)
            && note
            && input.mode == crate::video::models::VideoOutputMode::Note,
        note,
        mode: input.mode,
        force_transcribe: input.force_transcribe,
    };
    // 文本与视觉请求共用同一预算装饰器；单次底层调用没有叠加重试。
    let model = crate::services::video_budget::BudgetedModel {
        inner: &model,
        budget: &budget,
        provider_id: &input.provider_id,
        control: &control,
    };
    let deps = PipelineDeps {
        budget: &budget,
        client,
        tools,
        notes: &notes,
        model: &model,
        model_label,
        supports_vision,
        settings,
        vault_root,
        data_dir,
        resource_dir,
        manifest_dir,
    };
    let result = video_pipeline::run(deps, pipeline_input, control.clone()).await;
    execution.finish(&result);
    result.map(|_| control.snapshot())
}

/// 后台启动任务并立即返回快照（界面轮询）；用户手动发起，进入历史列表。
pub(crate) async fn start(
    app: &tauri::AppHandle,
    owner: &str,
    input: VideoStartInput,
) -> Result<VideoTaskStatus, CommandError> {
    let prepared = prepare(app, owner, input, true, TaskOrigin::User).await?;
    let status = prepared.control.snapshot();
    tauri::async_runtime::spawn(async move {
        if let Err(error) = execute(prepared).await {
            eprintln!("VIDEO_TASK(detail): {error}");
        }
    });
    Ok(status)
}

/// 同步等待任务完成（Agent 工具调用）；来源固定为 Agent，任务完成后按保留窗口清理。
pub(crate) async fn run_now(
    app: &tauri::AppHandle,
    owner: &str,
    input: VideoStartInput,
    note: bool,
) -> Result<VideoTaskStatus, CommandError> {
    let prepared = prepare(app, owner, input, note, TaskOrigin::Agent).await?;
    execution::wait(prepared.control.clone(), execute(prepared)).await
}
