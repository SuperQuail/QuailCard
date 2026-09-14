use std::{collections::HashMap, sync::Arc, time::Duration};

use reqwest::Client;
use tokio::sync::Mutex;

use tokio::net::TcpListener;

use super::{
    openai_oauth_helpers::{
        cancelled_error, read_oauth_credential, to_access, DeviceCodeResponse, LoginCancellation,
        OpenAiAccess, USER_AGENT,
    },
    AppServices,
};
use crate::{
    error::CommandError,
    models::{
        OpenAiLoginMode, OpenAiLoginStart, OpenAiLoginStatus, ProviderConfig, ProviderSummary,
        OPENAI_SUBSCRIPTION_PROVIDER_ID, OPENAI_SUBSCRIPTION_PROVIDER_TYPE,
    },
    storage::{now_timestamp, Storage},
    vault::EncryptedVault,
};

#[path = "openai_oauth_browser.rs"]
mod browser;
#[path = "openai_oauth_device.rs"]
mod device;

/// 管理 OpenAI OAuth 登录尝试和令牌刷新。
#[derive(Clone)]
pub(crate) struct OpenAiOAuthService {
    pub(super) client: Client,
    pub(super) attempts: Arc<Mutex<HashMap<String, AttemptRecord>>>,
}

/// 保存单次登录尝试的非敏感状态。
pub(super) struct AttemptRecord {
    output: OpenAiLoginStatus,
    updated_at: i64,
    cancellation: LoginCancellation,
}

/// 浏览器 OAuth 后台任务所需的完整上下文。
pub(super) struct BrowserLoginContext {
    pub(super) storage: Storage,
    pub(super) vault: EncryptedVault,
    pub(super) provider_id: String,
    pub(super) listener: TcpListener,
    pub(super) expected_state: String,
    pub(super) verifier: String,
    pub(super) attempt_id: String,
    pub(super) cancellation: LoginCancellation,
}

/// 设备码 OAuth 后台任务所需的完整上下文。
pub(super) struct DeviceLoginContext {
    pub(super) storage: Storage,
    pub(super) vault: EncryptedVault,
    pub(super) provider_id: String,
    pub(super) device: DeviceCodeResponse,
    pub(super) interval: Duration,
    pub(super) attempt_id: String,
    pub(super) cancellation: LoginCancellation,
}

impl AppServices {
    /// 启动 OpenAI OAuth 登录流程。
    pub async fn start_openai_login(
        &self,
        storage: &Storage,
        provider_id: &str,
        mode: OpenAiLoginMode,
    ) -> Result<OpenAiLoginStart, CommandError> {
        self.oauth
            .start(storage, &self.vault, provider_id, mode)
            .await
    }

    /// 查询 OpenAI OAuth 登录流程状态。
    pub async fn get_openai_login_status(
        &self,
        attempt_id: &str,
    ) -> Result<OpenAiLoginStatus, CommandError> {
        self.oauth.status(attempt_id).await
    }

    /// 取消尚未进入令牌写入阶段的 OpenAI OAuth 登录。
    pub async fn cancel_openai_login(
        &self,
        attempt_id: &str,
    ) -> Result<OpenAiLoginStatus, CommandError> {
        self.oauth.cancel(attempt_id).await
    }

    /// 注销指定供应商的 OpenAI OAuth 凭据。
    pub async fn logout_openai(
        &self,
        storage: &Storage,
        provider_id: &str,
    ) -> Result<ProviderSummary, CommandError> {
        self.oauth.logout(storage, &self.vault, provider_id).await
    }

    /// 为模型请求读取或刷新 OpenAI OAuth 访问令牌。
    pub(crate) async fn load_openai_access(
        &self,
        storage: &Storage,
        config: &ProviderConfig,
    ) -> Result<OpenAiAccess, CommandError> {
        self.oauth.access(storage, &self.vault, config).await
    }
}

impl OpenAiOAuthService {
    /// 创建配置了请求超时的网络客户端的 OAuth 服务。
    pub fn new() -> Result<Self, CommandError> {
        let client = Client::builder()
            .timeout(Duration::from_secs(30))
            .user_agent(USER_AGENT)
            .build()
            .map_err(|_| CommandError::new("OAUTH_CLIENT_ERROR", "无法初始化 OpenAI 登录客户端"))?;
        Ok(Self {
            client,
            attempts: Arc::new(Mutex::new(HashMap::new())),
        })
    }

    /// 启动浏览器 PKCE 或设备码登录，并在后台完成凭据切换。
    pub async fn start(
        &self,
        storage: &Storage,
        vault: &EncryptedVault,
        provider_id: &str,
        mode: OpenAiLoginMode,
    ) -> Result<OpenAiLoginStart, CommandError> {
        let provider = storage.get_provider_summary(provider_id).await?;
        if provider.id != OPENAI_SUBSCRIPTION_PROVIDER_ID
            || provider.provider_type != OPENAI_SUBSCRIPTION_PROVIDER_TYPE
            || provider.protocol != "OpenAI Compatible"
        {
            return Err(CommandError::validation(
                "ChatGPT 登录仅支持内置 OpenAI 订阅供应商",
            ));
        }
        let _guard = vault.lock_operations().await;
        if vault.status(storage).await?.locked {
            return Err(CommandError::new(
                "VAULT_LOCKED",
                "凭据保险库已锁定，请先输入密码解锁",
            ));
        }
        self.ensure_no_pending_attempt().await?;

        match mode {
            OpenAiLoginMode::Browser => {
                self.start_browser(storage.clone(), vault.clone(), provider_id.to_string())
                    .await
            }
            OpenAiLoginMode::Device => {
                self.start_device(storage.clone(), vault.clone(), provider_id.to_string())
                    .await
            }
        }
    }

    /// 返回指定登录尝试的最新非敏感状态。
    pub async fn status(&self, attempt_id: &str) -> Result<OpenAiLoginStatus, CommandError> {
        self.attempts
            .lock()
            .await
            .get(attempt_id)
            .map(|record| record.output.clone())
            .ok_or_else(|| CommandError::new("OAUTH_ATTEMPT_NOT_FOUND", "登录尝试不存在或已过期"))
    }

    /// 取消仍在等待用户授权的登录尝试。
    pub async fn cancel(&self, attempt_id: &str) -> Result<OpenAiLoginStatus, CommandError> {
        let mut attempts = self.attempts.lock().await;
        let record = attempts.get_mut(attempt_id).ok_or_else(|| {
            CommandError::new("OAUTH_ATTEMPT_NOT_FOUND", "登录尝试不存在或已过期")
        })?;
        match record.output.status.as_str() {
            "pending" => {
                record.cancellation.cancel();
                record.output.status = "cancelled".to_string();
                record.output.message = "已取消 ChatGPT 登录".to_string();
                record.updated_at = now_timestamp();
                Ok(record.output.clone())
            }
            "completing" => Err(CommandError::new(
                "OAUTH_LOGIN_COMPLETING",
                "授权已完成，正在安全写入凭据，当前不能取消",
            )),
            _ => Ok(record.output.clone()),
        }
    }

    /// 注销 OpenAI OAuth，并原子删除保险库密文和供应商引用。
    pub async fn logout(
        &self,
        storage: &Storage,
        vault: &EncryptedVault,
        provider_id: &str,
    ) -> Result<ProviderSummary, CommandError> {
        let _guard = vault.lock_operations().await;
        self.ensure_no_pending_attempt().await?;
        let config = storage
            .get_provider_config(provider_id)
            .await?
            .ok_or_else(|| CommandError::new("PROVIDER_NOT_FOUND", "供应商不存在"))?;
        if config.auth_type.as_deref() != Some("openai_oauth") {
            return Err(CommandError::validation("当前供应商未使用 ChatGPT 登录"));
        }
        if config.provider_type != OPENAI_SUBSCRIPTION_PROVIDER_TYPE {
            return Err(CommandError::new(
                "PROVIDER_AUTH_MISMATCH",
                "OAuth 凭据不属于 OpenAI 订阅供应商",
            ));
        }
        let secret_ref = config.secret_ref.as_deref().ok_or_else(|| {
            CommandError::new("PROVIDER_CREDENTIAL_MISSING", "OAuth 凭据引用不存在")
        })?;
        let envelope = vault.prepare_delete_credential(storage, secret_ref).await?;
        let provider = storage
            .replace_provider_credential(provider_id, None, None, None, &envelope)
            .await?;
        Ok(provider)
    }

    /// 读取有效访问令牌，并在临近过期时串行刷新和安全回写。
    pub async fn access(
        &self,
        storage: &Storage,
        vault: &EncryptedVault,
        config: &ProviderConfig,
    ) -> Result<OpenAiAccess, CommandError> {
        if config.auth_type.as_deref() != Some("openai_oauth") {
            return Err(CommandError::new(
                "PROVIDER_AUTH_MISMATCH",
                "供应商未使用 OpenAI OAuth",
            ));
        }
        let secret_ref = config.secret_ref.as_deref().ok_or_else(|| {
            CommandError::new("PROVIDER_CREDENTIAL_MISSING", "OAuth 凭据引用不存在")
        })?;
        let credential = read_oauth_credential(vault, storage, secret_ref).await?;
        if credential.expires_at > now_timestamp() + 60 {
            return Ok(to_access(credential));
        }

        let _guard = vault.lock_operations().await;
        let credential = read_oauth_credential(vault, storage, secret_ref).await?;
        if credential.expires_at > now_timestamp() + 60 {
            return Ok(to_access(credential));
        }
        self.refresh_and_persist(storage, vault, &config.id, secret_ref, &credential)
            .await
    }

    /// 阻止多个并发登录任务竞争固定回调端口或覆盖凭据。
    pub(crate) async fn ensure_no_pending_attempt(&self) -> Result<(), CommandError> {
        let now = now_timestamp();
        let mut attempts = self.attempts.lock().await;
        attempts.retain(|_, record| {
            matches!(record.output.status.as_str(), "pending" | "completing")
                || now - record.updated_at < 1800
        });
        if attempts
            .values()
            .any(|record| matches!(record.output.status.as_str(), "pending" | "completing"))
        {
            return Err(CommandError::new(
                "OAUTH_LOGIN_IN_PROGRESS",
                "已有 OpenAI 登录正在进行",
            ));
        }
        Ok(())
    }

    /// 插入不包含令牌或授权码的待处理状态。
    pub(super) async fn insert_pending(
        &self,
        attempt_id: &str,
        message: &str,
    ) -> LoginCancellation {
        let cancellation = LoginCancellation::new();
        self.attempts.lock().await.insert(
            attempt_id.to_string(),
            AttemptRecord {
                output: OpenAiLoginStatus {
                    status: "pending".to_string(),
                    message: message.to_string(),
                    provider: None,
                },
                updated_at: now_timestamp(),
                cancellation: cancellation.clone(),
            },
        );
        cancellation
    }

    /// 将已收到授权码的登录流程切换到不可取消的凭据写入阶段。
    pub(super) async fn mark_completing(&self, attempt_id: &str) -> Result<(), CommandError> {
        let mut attempts = self.attempts.lock().await;
        let record = attempts.get_mut(attempt_id).ok_or_else(|| {
            CommandError::new("OAUTH_ATTEMPT_NOT_FOUND", "登录尝试不存在或已过期")
        })?;
        if record.output.status == "cancelled" || record.cancellation.is_cancelled() {
            return Err(cancelled_error());
        }
        record.output.status = "completing".to_string();
        record.output.message = "授权已完成，正在安全写入凭据".to_string();
        record.updated_at = now_timestamp();
        Ok(())
    }

    /// 将后台登录结果写入可查询状态。
    pub(super) async fn finish_attempt(
        &self,
        attempt_id: &str,
        result: Result<ProviderSummary, CommandError>,
    ) {
        let output = match result {
            Ok(provider) => OpenAiLoginStatus {
                status: "success".to_string(),
                message: "ChatGPT 登录成功".to_string(),
                provider: Some(provider),
            },
            Err(error) => OpenAiLoginStatus {
                status: "failed".to_string(),
                message: error.message,
                provider: None,
            },
        };
        let mut attempts = self.attempts.lock().await;
        if let Some(record) = attempts.get_mut(attempt_id) {
            if record.output.status != "cancelled" {
                record.output = output;
                record.updated_at = now_timestamp();
            }
        }
    }
}

#[cfg(test)]
#[path = "openai_oauth_tests.rs"]
mod tests;
