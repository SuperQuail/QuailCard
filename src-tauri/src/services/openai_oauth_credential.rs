//! OpenAI OAuth 凭据的持久化与刷新回写编排。
//!
//! 本模块承接登录完成与令牌刷新后把凭据安全写入加密保险库、并原子替换数据库
//! 引用的编排逻辑。它依赖 repository 方法（`Database`）与保险库端口
//! （`EncryptedVault`），但不依赖任何协议端点细节；令牌交换与刷新本身在
//! `openai_oauth_token` 模块。

use uuid::Uuid;

use super::openai_oauth::OpenAiOAuthService;
use super::openai_oauth_helpers::{credential_from_tokens, to_access, OpenAiAccess, TokenResponse};
use super::openai_oauth_token::refresh_tokens;
use crate::{
    database::Database,
    error::CommandError,
    models::{
        OpenAiOAuthCredential, ProviderSummary, OPENAI_SUBSCRIPTION_PROVIDER_ID,
        OPENAI_SUBSCRIPTION_PROVIDER_TYPE,
    },
    vault::EncryptedVault,
};

impl OpenAiOAuthService {
    /// 将新获得的 OAuth 令牌序列化后写入加密保险库并原子替换数据库引用。
    ///
    /// 写入前先在保险库写锁内复验供应商仍是内置 OpenAI 订阅且协议未变化，
    /// 避免登录期间配置被并发改动后写坏凭据。序列化密文经
    /// `prepare_set_credential` 处理，原始令牌只存在于局部变量，不落日志。
    pub(super) async fn persist_credential(
        &self,
        database: &Database,
        vault: &EncryptedVault,
        provider_id: &str,
        tokens: TokenResponse,
    ) -> Result<ProviderSummary, CommandError> {
        let _guard = vault.lock_operations().await;
        let credential = credential_from_tokens(tokens, None)?;
        let serialized = serde_json::to_string(&credential)
            .map_err(|_| CommandError::new("OAUTH_CREDENTIAL_ERROR", "无法序列化 OAuth 凭据"))?;
        let current = database
            .get_provider_config(provider_id)
            .await?
            .ok_or_else(|| CommandError::new("PROVIDER_NOT_FOUND", "供应商不存在"))?;
        if current.id != OPENAI_SUBSCRIPTION_PROVIDER_ID
            || current.provider_type != OPENAI_SUBSCRIPTION_PROVIDER_TYPE
            || current.protocol != "OpenAI Compatible"
        {
            return Err(CommandError::validation(
                "登录期间供应商协议已变化，请重新发起 ChatGPT 登录",
            ));
        }
        let new_secret_ref = Uuid::now_v7().to_string();
        let envelope = vault
            .prepare_set_credential(
                database,
                &new_secret_ref,
                &serialized,
                current.secret_ref.as_deref(),
            )
            .await?;
        let provider = database
            .replace_provider_credential(
                provider_id,
                Some(&new_secret_ref),
                Some("openai_oauth"),
                credential.account_id.as_deref(),
                &envelope,
            )
            .await?;
        Ok(provider)
    }

    /// 用 refresh token 刷新访问令牌并安全回写保险库。
    ///
    /// 在保险库写锁内执行：刷新成功后保留原账号 ID（新响应可能缺失该声明），
    /// 序列化回写密文，再原子更新数据库账号摘要。返回裁剪后的访问凭据，
    /// 不暴露 refresh token；所有敏感值仅存于局部变量，不进入日志或错误消息。
    pub(super) async fn refresh_and_persist(
        &self,
        database: &Database,
        vault: &EncryptedVault,
        provider_id: &str,
        secret_ref: &str,
        credential: &OpenAiOAuthCredential,
    ) -> Result<OpenAiAccess, CommandError> {
        let tokens = refresh_tokens(&self.client, &credential.refresh_token).await?;
        let previous_account_id = credential.account_id.clone();
        let mut refreshed = credential_from_tokens(tokens, Some(&credential.refresh_token))?;
        if refreshed.account_id.is_none() {
            refreshed.account_id = previous_account_id;
        }
        let serialized = serde_json::to_string(&refreshed).map_err(|_| {
            CommandError::new("OAUTH_CREDENTIAL_ERROR", "无法序列化刷新后的 OAuth 凭据")
        })?;
        let envelope = vault
            .prepare_set_credential(database, secret_ref, &serialized, None)
            .await?;
        database
            .save_refreshed_oauth_credential(
                provider_id,
                refreshed.account_id.as_deref(),
                &envelope,
            )
            .await?;
        Ok(to_access(refreshed))
    }
}
