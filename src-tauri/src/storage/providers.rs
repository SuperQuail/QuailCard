//! providers.toml：模型供应商非敏感配置的查询与写透。
//!
//! 凭据密文保存在 vault.bin（见 vault_file），本文件只保存随机引用
//! （secretRef）与非敏感字段；新增供应商能力一律增加可选字段
//! （文件结构定义见 providers_file 模块）。

use super::{
    envelope, now_timestamp,
    providers_file::{ProviderRecord, ProvidersFile, FILE_NAME},
    Storage,
};
use crate::{
    error::CommandError,
    models::{ProviderConfig, ProviderInput, ProviderSummary, VaultEnvelope},
};

impl Storage {
    /// 查询全部供应商非敏感摘要，按创建时间与名称排序。
    pub async fn list_providers(&self) -> Result<Vec<ProviderSummary>, CommandError> {
        let file = self.inner.providers.lock().await;
        let mut records: Vec<&ProviderRecord> = file.providers.iter().collect();
        records.sort_by(|left, right| {
            left.created_at
                .cmp(&right.created_at)
                .then_with(|| left.name.cmp(&right.name))
        });
        Ok(records.into_iter().map(summary_of).collect())
    }

    /// 获取当前活动供应商标识。
    pub async fn get_active_provider_id(&self) -> Result<String, CommandError> {
        self.inner
            .providers
            .lock()
            .await
            .active_provider_id
            .clone()
            .ok_or_else(|| CommandError::new("PROVIDER_NOT_FOUND", "活动供应商不存在"))
    }

    /// 更新当前活动供应商，目标不存在时拒绝。
    pub async fn set_active_provider(&self, provider_id: &str) -> Result<(), CommandError> {
        let mut file = self.inner.providers.lock().await;
        if !file.providers.iter().any(|record| record.id == provider_id) {
            return Err(CommandError::new("PROVIDER_NOT_FOUND", "供应商不存在"));
        }
        file.active_provider_id = Some(provider_id.to_string());
        self.persist_providers(&file)
    }

    /// 查询模型请求使用的供应商配置和凭据引用。
    pub async fn get_provider_config(
        &self,
        provider_id: &str,
    ) -> Result<Option<ProviderConfig>, CommandError> {
        let file = self.inner.providers.lock().await;
        Ok(file
            .providers
            .iter()
            .find(|record| record.id == provider_id)
            .map(config_of))
    }

    /// 查询当前活动供应商的模型请求配置。
    pub async fn get_active_provider_config(&self) -> Result<ProviderConfig, CommandError> {
        let provider_id = self.get_active_provider_id().await?;
        self.get_provider_config(&provider_id)
            .await?
            .ok_or_else(|| CommandError::new("PROVIDER_NOT_FOUND", "活动供应商不存在"))
    }

    /// 创建或更新供应商配置并切换凭据引用。
    ///
    /// 若携带新保险库密文则先写 vault.bin 再写 providers.toml：
    /// 崩溃时最多留下未被引用的孤儿密文，不会出现悬空引用。
    pub async fn save_provider_config(
        &self,
        input: &ProviderInput,
        provider_id: &str,
        secret_ref: Option<&str>,
        auth_type: Option<&str>,
        oauth_account_id: Option<&str>,
        vault_envelope: Option<&VaultEnvelope>,
    ) -> Result<ProviderSummary, CommandError> {
        validate_provider(input)?;
        validate_credential_metadata(secret_ref, auth_type, oauth_account_id)?;
        if let Some(envelope_value) = vault_envelope {
            self.save_vault_envelope(envelope_value).await?;
        }
        let now = now_timestamp();
        let mut file = self.inner.providers.lock().await;
        let existing = file
            .providers
            .iter()
            .find(|record| record.id == provider_id)
            .cloned();
        let record = ProviderRecord {
            id: provider_id.to_string(),
            name: input.name.trim().to_string(),
            short_code: input.short_code.trim().to_string(),
            protocol: input.protocol.trim().to_string(),
            model: input.model.trim().to_string(),
            base_url: input.base_url.trim().to_string(),
            has_api_key: auth_type == Some("api_key"),
            supports_vision: input.supports_vision,
            status: "untested".to_string(),
            secret_ref: secret_ref.map(str::to_string),
            auth_type: auth_type.map(str::to_string),
            oauth_account_id: oauth_account_id.map(str::to_string),
            provider_type: existing
                .as_ref()
                .map(|record| record.provider_type.clone())
                .unwrap_or_else(|| "api".to_string()),
            created_at: existing.as_ref().map_or(now, |record| record.created_at),
            updated_at: now,
        };
        let summary = summary_of(&record);
        upsert_record(&mut file, record);
        self.persist_providers(&file)?;
        Ok(summary)
    }

    /// 查询单个供应商的非敏感摘要。
    pub async fn get_provider_summary(
        &self,
        provider_id: &str,
    ) -> Result<ProviderSummary, CommandError> {
        let file = self.inner.providers.lock().await;
        file.providers
            .iter()
            .find(|record| record.id == provider_id)
            .map(summary_of)
            .ok_or_else(|| CommandError::new("PROVIDER_NOT_FOUND", "供应商不存在"))
    }

    /// 原子切换供应商凭据元数据和加密保险库密文记录。
    pub async fn replace_provider_credential(
        &self,
        provider_id: &str,
        secret_ref: Option<&str>,
        auth_type: Option<&str>,
        oauth_account_id: Option<&str>,
        vault_envelope: &VaultEnvelope,
    ) -> Result<ProviderSummary, CommandError> {
        validate_credential_metadata(secret_ref, auth_type, oauth_account_id)?;
        self.save_vault_envelope(vault_envelope).await?;
        let mut file = self.inner.providers.lock().await;
        let now = now_timestamp();
        let Some(record) = file
            .providers
            .iter_mut()
            .find(|record| record.id == provider_id)
        else {
            return Err(CommandError::new("PROVIDER_NOT_FOUND", "供应商不存在"));
        };
        record.secret_ref = secret_ref.map(str::to_string);
        record.auth_type = auth_type.map(str::to_string);
        record.oauth_account_id = oauth_account_id.map(str::to_string);
        record.has_api_key = auth_type == Some("api_key");
        record.status = "untested".to_string();
        record.updated_at = now;
        let summary = summary_of(record);
        self.persist_providers(&file)?;
        Ok(summary)
    }

    /// 原子保存刷新后的 OAuth 密文和账号摘要。
    pub async fn save_refreshed_oauth_credential(
        &self,
        provider_id: &str,
        account_id: Option<&str>,
        vault_envelope: &VaultEnvelope,
    ) -> Result<(), CommandError> {
        self.save_vault_envelope(vault_envelope).await?;
        let mut file = self.inner.providers.lock().await;
        if let Some(record) = file.providers.iter_mut().find(|record| {
            record.id == provider_id && record.auth_type.as_deref() == Some("openai_oauth")
        }) {
            record.oauth_account_id = account_id.map(str::to_string);
            record.updated_at = now_timestamp();
        }
        self.persist_providers(&file)
    }

    /// 更新连接测试状态并返回最新摘要。
    pub async fn set_provider_connected(
        &self,
        provider_id: &str,
    ) -> Result<ProviderSummary, CommandError> {
        let mut file = self.inner.providers.lock().await;
        let Some(record) = file
            .providers
            .iter_mut()
            .find(|record| record.id == provider_id)
        else {
            return Err(CommandError::new("PROVIDER_NOT_FOUND", "供应商不存在"));
        };
        record.status = "connected".to_string();
        record.updated_at = now_timestamp();
        let summary = summary_of(record);
        self.persist_providers(&file)?;
        Ok(summary)
    }

    /// 删除供应商，并在同一操作中写回清理凭据后的保险库密文。
    pub async fn delete_provider(
        &self,
        provider_id: &str,
        envelope_value: Option<&VaultEnvelope>,
    ) -> Result<(), CommandError> {
        if let Some(envelope_value) = envelope_value {
            self.save_vault_envelope(envelope_value).await?;
        }
        let mut file = self.inner.providers.lock().await;
        let before = file.providers.len();
        file.providers.retain(|record| record.id != provider_id);
        if file.providers.len() == before {
            return Err(CommandError::new("PROVIDER_NOT_FOUND", "供应商不存在"));
        }
        self.persist_providers(&file)
    }

    /// 重置保险库并清除所有供应商的失效凭据引用。
    pub async fn reset_credential_vault(
        &self,
        envelope_value: &VaultEnvelope,
    ) -> Result<(), CommandError> {
        self.save_vault_envelope(envelope_value).await?;
        let mut file = self.inner.providers.lock().await;
        let now = now_timestamp();
        for record in &mut file.providers {
            record.secret_ref = None;
            record.auth_type = None;
            record.oauth_account_id = None;
            record.has_api_key = false;
            record.status = "untested".to_string();
            record.updated_at = now;
        }
        self.persist_providers(&file)
    }

    /// 把供应商内存镜像写透到 providers.toml。
    fn persist_providers(&self, file: &ProvidersFile) -> Result<(), CommandError> {
        envelope::save_toml(&self.inner.config_dir.join(FILE_NAME), file)
    }
}

/// 按 id 替换或追加记录，保持文件中的既有顺序。
fn upsert_record(file: &mut ProvidersFile, record: ProviderRecord) {
    match file
        .providers
        .iter()
        .position(|existing| existing.id == record.id)
    {
        Some(index) => file.providers[index] = record,
        None => file.providers.push(record),
    }
}

/// 把持久化记录转换为前端可见的非敏感摘要。
fn summary_of(record: &ProviderRecord) -> ProviderSummary {
    ProviderSummary {
        id: record.id.clone(),
        name: record.name.clone(),
        short_code: record.short_code.clone(),
        protocol: record.protocol.clone(),
        model: record.model.clone(),
        base_url: record.base_url.clone(),
        has_api_key: record.has_api_key,
        has_credential: record.secret_ref.is_some(),
        auth_type: record.auth_type.clone(),
        oauth_account_id: record.oauth_account_id.clone(),
        provider_type: record.provider_type.clone(),
        supports_vision: record.supports_vision,
        status: record.status.clone(),
    }
}

/// 把持久化记录转换为模型请求配置。
fn config_of(record: &ProviderRecord) -> ProviderConfig {
    ProviderConfig {
        id: record.id.clone(),
        protocol: record.protocol.clone(),
        model: record.model.clone(),
        base_url: record.base_url.clone(),
        secret_ref: record.secret_ref.clone(),
        auth_type: record.auth_type.clone(),
        oauth_account_id: record.oauth_account_id.clone(),
        provider_type: record.provider_type.clone(),
        supports_vision: record.supports_vision,
    }
}

/// 校验供应商非敏感配置。
fn validate_provider(input: &ProviderInput) -> Result<(), CommandError> {
    if input.name.trim().is_empty()
        || input.short_code.trim().is_empty()
        || input.protocol.trim().is_empty()
        || input.model.trim().is_empty()
        || input.base_url.trim().is_empty()
    {
        return Err(CommandError::validation("供应商配置存在空字段"));
    }
    if !input.base_url.starts_with("https://")
        && !input.base_url.starts_with("http://localhost")
        && !input.base_url.starts_with("http://127.0.0.1")
    {
        return Err(CommandError::validation(
            "BaseURL 必须使用 HTTPS，本机服务可以使用 HTTP",
        ));
    }
    Ok(())
}

/// 校验凭据引用、认证类型和账号摘要之间的一致性。
fn validate_credential_metadata(
    secret_ref: Option<&str>,
    auth_type: Option<&str>,
    oauth_account_id: Option<&str>,
) -> Result<(), CommandError> {
    let valid_pair = matches!(
        (secret_ref, auth_type),
        (None, None) | (Some(_), Some("api_key" | "openai_oauth"))
    );
    if !valid_pair || (auth_type != Some("openai_oauth") && oauth_account_id.is_some()) {
        return Err(CommandError::validation("供应商凭据元数据不一致"));
    }
    Ok(())
}

#[cfg(test)]
#[path = "providers_tests.rs"]
mod tests;
