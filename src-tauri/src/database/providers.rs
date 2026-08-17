use super::{now_timestamp, vault::upsert_vault_envelope, Database};
use crate::{
    error::CommandError,
    models::{ProviderConfig, ProviderInput, ProviderSummary, VaultEnvelope},
};

impl Database {
    /// 查询全部供应商非敏感摘要。
    pub async fn list_providers(&self) -> Result<Vec<ProviderSummary>, CommandError> {
        let providers = sqlx::query_as::<_, ProviderSummary>(
            "SELECT id, name, short_code, protocol, model, base_url,
                    has_api_key, secret_ref IS NOT NULL AS has_credential,
                    auth_type, oauth_account_id, provider_type, supports_vision, status
             FROM providers ORDER BY created_at, name",
        )
        .fetch_all(&self.pool)
        .await?;
        Ok(providers)
    }

    /// 获取当前活动供应商标识。
    pub async fn get_active_provider_id(&self) -> Result<String, CommandError> {
        let provider_id = sqlx::query_scalar::<_, String>(
            "SELECT value FROM app_settings WHERE key = 'active_provider_id'",
        )
        .fetch_one(&self.pool)
        .await?;
        Ok(provider_id)
    }

    /// 更新当前活动供应商。
    pub async fn set_active_provider(&self, provider_id: &str) -> Result<(), CommandError> {
        let exists = sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM providers WHERE id = ?")
            .bind(provider_id)
            .fetch_one(&self.pool)
            .await?;
        if exists == 0 {
            return Err(CommandError::new("PROVIDER_NOT_FOUND", "供应商不存在"));
        }
        sqlx::query(
            "INSERT INTO app_settings (key, value, updated_at)
             VALUES ('active_provider_id', ?, ?)
             ON CONFLICT(key) DO UPDATE SET value = excluded.value, updated_at = excluded.updated_at",
        )
        .bind(provider_id)
        .bind(now_timestamp())
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    /// 查询模型请求使用的供应商配置和凭据引用。
    pub async fn get_provider_config(
        &self,
        provider_id: &str,
    ) -> Result<Option<ProviderConfig>, CommandError> {
        sqlx::query_as::<_, ProviderConfig>(
            "SELECT id, protocol, model, base_url, secret_ref, auth_type, oauth_account_id,
                    provider_type, supports_vision
             FROM providers WHERE id = ?",
        )
        .bind(provider_id)
        .fetch_optional(&self.pool)
        .await
        .map_err(CommandError::from)
    }

    /// 查询当前活动供应商的模型请求配置。
    pub async fn get_active_provider_config(&self) -> Result<ProviderConfig, CommandError> {
        let provider_id = self.get_active_provider_id().await?;
        self.get_provider_config(&provider_id)
            .await?
            .ok_or_else(|| CommandError::new("PROVIDER_NOT_FOUND", "活动供应商不存在"))
    }

    /// 创建或更新供应商配置并原子切换凭据引用。
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
        let now = now_timestamp();
        let mut transaction = self.pool.begin().await?;
        if let Some(envelope) = vault_envelope {
            upsert_vault_envelope(&mut transaction, envelope).await?;
        }
        sqlx::query(
            "INSERT INTO providers (
                id, name, short_code, protocol, model, base_url,
                has_api_key, supports_vision, status, created_at, updated_at, secret_ref,
                auth_type, oauth_account_id
             ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, 'untested', ?, ?, ?, ?, ?)
             ON CONFLICT(id) DO UPDATE SET
                name = excluded.name, short_code = excluded.short_code,
                protocol = excluded.protocol, model = excluded.model,
                base_url = excluded.base_url, supports_vision = excluded.supports_vision,
                has_api_key = excluded.has_api_key, secret_ref = excluded.secret_ref,
                auth_type = excluded.auth_type, oauth_account_id = excluded.oauth_account_id,
                status = 'untested', updated_at = excluded.updated_at",
        )
        .bind(provider_id)
        .bind(input.name.trim())
        .bind(input.short_code.trim())
        .bind(input.protocol.trim())
        .bind(input.model.trim())
        .bind(input.base_url.trim())
        .bind(auth_type == Some("api_key"))
        .bind(input.supports_vision)
        .bind(now)
        .bind(now)
        .bind(secret_ref)
        .bind(auth_type)
        .bind(oauth_account_id)
        .execute(&mut *transaction)
        .await?;
        transaction.commit().await?;
        self.get_provider_summary(provider_id).await
    }

    /// 查询单个供应商的非敏感摘要。
    pub async fn get_provider_summary(
        &self,
        provider_id: &str,
    ) -> Result<ProviderSummary, CommandError> {
        sqlx::query_as::<_, ProviderSummary>(
            "SELECT id, name, short_code, protocol, model, base_url,
                     has_api_key, secret_ref IS NOT NULL AS has_credential,
                     auth_type, oauth_account_id, provider_type, supports_vision, status
             FROM providers WHERE id = ?",
        )
        .bind(provider_id)
        .fetch_one(&self.pool)
        .await
        .map_err(CommandError::from)
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
        let now = now_timestamp();
        let mut transaction = self.pool.begin().await?;
        upsert_vault_envelope(&mut transaction, vault_envelope).await?;
        let result = sqlx::query(
            "UPDATE providers
             SET secret_ref = ?, auth_type = ?, oauth_account_id = ?, has_api_key = ?,
                 status = 'untested', updated_at = ?
             WHERE id = ?",
        )
        .bind(secret_ref)
        .bind(auth_type)
        .bind(oauth_account_id)
        .bind(auth_type == Some("api_key"))
        .bind(now)
        .bind(provider_id)
        .execute(&mut *transaction)
        .await?;
        if result.rows_affected() == 0 {
            return Err(CommandError::new("PROVIDER_NOT_FOUND", "供应商不存在"));
        }
        transaction.commit().await?;
        self.get_provider_summary(provider_id).await
    }

    /// 原子保存刷新后的 OAuth 密文和账号摘要。
    pub async fn save_refreshed_oauth_credential(
        &self,
        provider_id: &str,
        account_id: Option<&str>,
        vault_envelope: &VaultEnvelope,
    ) -> Result<(), CommandError> {
        let mut transaction = self.pool.begin().await?;
        upsert_vault_envelope(&mut transaction, vault_envelope).await?;
        sqlx::query(
            "UPDATE providers SET oauth_account_id = ?, updated_at = ?
             WHERE id = ? AND auth_type = 'openai_oauth'",
        )
        .bind(account_id)
        .bind(now_timestamp())
        .bind(provider_id)
        .execute(&mut *transaction)
        .await?;
        transaction.commit().await?;
        Ok(())
    }

    /// 更新连接测试状态并返回最新摘要。
    pub async fn set_provider_connected(
        &self,
        provider_id: &str,
    ) -> Result<ProviderSummary, CommandError> {
        sqlx::query("UPDATE providers SET status = 'connected', updated_at = ? WHERE id = ?")
            .bind(now_timestamp())
            .bind(provider_id)
            .execute(&self.pool)
            .await?;
        self.get_provider_summary(provider_id).await
    }

    /// 删除供应商，并在同一事务中写回清理凭据后的保险库密文。
    pub async fn delete_provider(
        &self,
        provider_id: &str,
        envelope: Option<&VaultEnvelope>,
    ) -> Result<(), CommandError> {
        let mut transaction = self.pool.begin().await?;
        let result = sqlx::query("DELETE FROM providers WHERE id = ?")
            .bind(provider_id)
            .execute(&mut *transaction)
            .await?;
        if result.rows_affected() == 0 {
            return Err(CommandError::new("PROVIDER_NOT_FOUND", "供应商不存在"));
        }
        if let Some(envelope) = envelope {
            super::vault::upsert_vault_envelope(&mut transaction, envelope).await?;
        }
        transaction.commit().await?;
        Ok(())
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
mod tests {
    use super::*;

    #[tokio::test]
    /// 初始迁移会创建默认供应商和活动配置。
    async fn loads_default_providers() {
        let database = Database::connect_memory()
            .await
            .expect("创建测试数据库失败");
        let providers = database.list_providers().await.expect("查询供应商失败");
        assert_eq!(providers.len(), 4);
        assert!(providers
            .iter()
            .all(|provider| provider.status == "untested"));
        assert!(providers.iter().all(|provider| !provider.has_credential));
        assert!(providers
            .iter()
            .all(|provider| provider.auth_type.is_none()));
        let subscription = providers
            .iter()
            .find(|provider| provider.id == "openai_subscription")
            .expect("缺少 OpenAI 订阅供应商");
        assert_eq!(subscription.provider_type, "openai_subscription");
        assert_eq!(subscription.model, "gpt-5.5");
        assert!(subscription.supports_vision);
        let opencode_go = providers
            .iter()
            .find(|provider| provider.id == "opencode_go")
            .expect("缺少 OpenCode Go 供应商");
        assert_eq!(opencode_go.protocol, "OpenAI Compatible");
        assert_eq!(opencode_go.model, "deepseek-v4-flash");
        assert_eq!(opencode_go.base_url, "https://opencode.ai/zen/go/v1");
        assert!(!opencode_go.supports_vision);
        assert_eq!(
            database
                .get_active_provider_id()
                .await
                .expect("查询活动供应商失败"),
            "openai"
        );
    }

    #[tokio::test]
    /// OAuth 凭据切换只在数据库保存类型、账号摘要和随机引用。
    async fn replaces_oauth_credential_metadata() {
        let database = Database::connect_memory()
            .await
            .expect("创建测试数据库失败");
        let vault = crate::vault::EncryptedVault::new();
        vault.initialize(&database).await.expect("初始化保险库失败");
        let envelope = vault
            .prepare_set_credential(&database, "secret-1", "oauth-json", None)
            .await
            .expect("准备加密凭据失败");
        let provider = database
            .replace_provider_credential(
                "openai_subscription",
                Some("secret-1"),
                Some("openai_oauth"),
                Some("account-1"),
                &envelope,
            )
            .await
            .expect("切换 OAuth 凭据失败");
        assert!(provider.has_credential);
        assert!(!provider.has_api_key);
        assert_eq!(provider.auth_type.as_deref(), Some("openai_oauth"));
        assert_eq!(provider.oauth_account_id.as_deref(), Some("account-1"));
        assert_eq!(
            vault
                .get_credential(&database, "secret-1")
                .await
                .expect("读取加密凭据失败"),
            "oauth-json"
        );
        assert!(database
            .get_vault_envelope()
            .await
            .expect("查询保险库失败")
            .is_some());
    }
}
