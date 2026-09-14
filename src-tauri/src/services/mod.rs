pub(crate) mod agent;
pub(crate) mod agent_events;
pub(crate) mod agent_goal;
pub(crate) mod agent_ports;
pub(crate) mod agent_tasks;
pub(crate) mod agent_write_scope;
mod ai_generation;
mod ai_tasks;
mod generation_dictionary;
mod generation_executor;
pub(crate) mod generation_ports;
mod generation_round;
mod generation_tasks;
mod openai_oauth;
mod openai_oauth_credential;
mod openai_oauth_helpers;
mod openai_oauth_token;
mod providers;
mod speech;
pub(crate) mod subagents;
mod turn_loop;
mod vault;
pub(crate) mod video_budget;
pub(crate) mod video_login;
pub(crate) mod video_note;
pub(crate) mod video_pipeline;
pub(crate) mod video_ports;
pub(crate) mod video_tasks;
mod video_work;

use crate::{
    ai::ProviderGateway, error::CommandError, models::ProviderConfig, storage::Storage,
    vault::EncryptedVault,
};
use openai_oauth::OpenAiOAuthService;

pub(crate) use generation_tasks::{GenerationControl, GenerationTaskRegistry};
pub use speech::SpeechService;
pub(crate) use video_login::VideoLoginSessions;
pub(crate) use video_tasks::{VideoDownloads, VideoTaskRegistry};

/// 组合模型客户端和加密凭据保险库的应用服务。
pub struct AppServices {
    pub(crate) ai: ProviderGateway,
    pub(crate) vault: EncryptedVault,
    pub(crate) oauth: OpenAiOAuthService,
    pub(crate) generation_tasks: GenerationTaskRegistry,
    /// 视频任务注册表（每窗口只允许一个运行中任务）。
    pub(crate) video_tasks: VideoTaskRegistry,
    /// 所有视频任务共享资源额度；克隆不会创建新的限流器。
    pub(crate) video_budget: video_budget::VideoBudget,
    /// 扫码登录会话表。
    pub(crate) video_login: VideoLoginSessions,
    /// 模型与加速包下载的取消表。
    pub(crate) video_downloads: VideoDownloads,
}

impl AppServices {
    /// 创建应用进程复用的服务实例。
    pub fn new() -> Result<Self, CommandError> {
        Ok(Self {
            ai: ProviderGateway::new()?,
            vault: EncryptedVault::new(),
            oauth: OpenAiOAuthService::new()?,
            generation_tasks: GenerationTaskRegistry::default(),
            video_tasks: VideoTaskRegistry::default(),
            video_budget: video_budget::VideoBudget::default(),
            video_login: VideoLoginSessions::default(),
            video_downloads: VideoDownloads::default(),
        })
    }

    /// 启动时确保唯一加密保险库已经初始化。
    pub async fn initialize(&self, storage: &Storage) -> Result<(), CommandError> {
        self.vault.initialize(storage).await
    }

    /// 读取供应商配置所引用的 API Key。
    pub(crate) async fn load_api_key(
        &self,
        storage: &Storage,
        config: &ProviderConfig,
    ) -> Result<String, CommandError> {
        if config.auth_type.as_deref() != Some("api_key") {
            return Err(CommandError::new(
                "PROVIDER_AUTH_MISMATCH",
                "供应商未使用 API Key 认证",
            ));
        }
        let secret_ref = config
            .secret_ref
            .as_deref()
            .ok_or_else(|| CommandError::new("PROVIDER_KEY_MISSING", "请先配置 API Key"))?;
        self.vault.get_credential(storage, secret_ref).await
    }
}
