mod ai_generation;
mod ai_tasks;
mod openai_oauth;
mod openai_oauth_credential;
mod openai_oauth_helpers;
mod openai_oauth_token;
mod providers;
mod speech;
mod vault;

use crate::{
    ai::AiClient, error::CommandError, models::ProviderConfig, storage::Storage,
    vault::EncryptedVault,
};
use openai_oauth::OpenAiOAuthService;

pub use speech::SpeechService;

/// 组合模型客户端和加密凭据保险库的应用服务。
pub struct AppServices {
    pub(crate) ai: AiClient,
    pub(crate) vault: EncryptedVault,
    pub(crate) oauth: OpenAiOAuthService,
}

impl AppServices {
    /// 创建应用进程复用的服务实例。
    pub fn new() -> Result<Self, CommandError> {
        Ok(Self {
            ai: AiClient::new()?,
            vault: EncryptedVault::new(),
            oauth: OpenAiOAuthService::new()?,
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
