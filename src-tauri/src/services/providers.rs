use std::time::Instant;

use reqwest::Url;
use uuid::Uuid;

use super::AppServices;
use crate::{
    ai::{normalize_base_url, ProviderProtocol},
    database::Database,
    error::CommandError,
    models::{
        ConnectionTestResult, ProviderConfig, ProviderInput, ProviderSummary,
        OPENAI_SUBSCRIPTION_ENDPOINT, OPENAI_SUBSCRIPTION_PROVIDER_ID,
        OPENAI_SUBSCRIPTION_PROVIDER_TYPE,
    },
};

impl AppServices {
    /// 保存供应商配置，并在一个 SQLite 事务内切换加密凭据。
    pub async fn save_provider(
        &self,
        database: &Database,
        mut input: ProviderInput,
    ) -> Result<ProviderSummary, CommandError> {
        normalize_provider_input(&mut input)?;
        let _guard = self.vault.lock_operations().await;
        self.oauth.ensure_no_pending_attempt().await?;
        let provider_id = input
            .id
            .clone()
            .unwrap_or_else(|| Uuid::now_v7().to_string());
        let current = database.get_provider_config(&provider_id).await?;
        let new_api_key = take_new_api_key(&mut input)?;
        validate_provider_subtype(
            current.as_ref(),
            &provider_id,
            &input,
            new_api_key.as_deref(),
        )?;
        ensure_safe_secret_reuse(current.as_ref(), &input, new_api_key.as_deref())?;

        let old_secret_ref = current
            .as_ref()
            .and_then(|config| config.secret_ref.clone());
        let Some(api_key) = new_api_key else {
            let auth_type = current
                .as_ref()
                .and_then(|config| config.auth_type.as_deref());
            let oauth_account_id = current
                .as_ref()
                .and_then(|config| config.oauth_account_id.as_deref());
            return database
                .save_provider_config(
                    &input,
                    &provider_id,
                    old_secret_ref.as_deref(),
                    auth_type,
                    oauth_account_id,
                    None,
                )
                .await;
        };

        let new_secret_ref = Uuid::now_v7().to_string();
        let envelope = self
            .vault
            .prepare_set_credential(
                database,
                &new_secret_ref,
                &api_key,
                old_secret_ref.as_deref(),
            )
            .await?;
        database
            .save_provider_config(
                &input,
                &provider_id,
                Some(&new_secret_ref),
                Some("api_key"),
                None,
                Some(&envelope),
            )
            .await
    }

    /// 删除供应商及其加密凭据。
    pub async fn delete_provider(
        &self,
        database: &Database,
        provider_id: &str,
    ) -> Result<(), CommandError> {
        let _guard = self.vault.lock_operations().await;
        self.oauth.ensure_no_pending_attempt().await?;
        let current = database.get_provider_config(provider_id).await?;
        let envelope = match current
            .as_ref()
            .and_then(|config| config.secret_ref.as_deref())
        {
            Some(secret_ref) => Some(
                self.vault
                    .prepare_delete_credential(database, secret_ref)
                    .await?,
            ),
            None => None,
        };
        database
            .delete_provider(provider_id, envelope.as_ref())
            .await
    }

    /// 使用当前表单配置发起真实连接测试，且绝不回传密钥。
    pub async fn test_provider(
        &self,
        database: &Database,
        mut input: ProviderInput,
    ) -> Result<ConnectionTestResult, CommandError> {
        normalize_provider_input(&mut input)?;
        let current = match input.id.as_deref() {
            Some(provider_id) => database.get_provider_config(provider_id).await?,
            None => None,
        };
        let supplied_key = take_new_api_key(&mut input)?;
        ensure_safe_secret_reuse(current.as_ref(), &input, supplied_key.as_deref())?;
        let config = provider_config_from_input(&input, current.as_ref());

        let started_at = Instant::now();
        match supplied_key.as_deref() {
            Some(api_key) => self.ai.test_connection(&config, api_key).await?,
            None => {
                let stored = current.as_ref().ok_or_else(missing_credential)?;
                match stored.auth_type.as_deref() {
                    Some("openai_oauth") => {
                        let access = self.load_openai_access(database, stored).await?;
                        self.ai
                            .test_openai_oauth_connection(
                                &config,
                                &access.access_token,
                                access.account_id.as_deref(),
                            )
                            .await?;
                    }
                    Some("api_key") => {
                        let api_key = self.load_api_key(database, stored).await?;
                        self.ai.test_connection(&config, &api_key).await?;
                    }
                    _ => return Err(missing_credential()),
                }
            }
        }
        let persisted_match = supplied_key.is_none()
            && current
                .as_ref()
                .is_some_and(|stored| same_model_config(stored, &config));
        let provider = if persisted_match {
            database.set_provider_connected(&config.id).await.ok()
        } else {
            None
        };
        Ok(ConnectionTestResult {
            latency_ms: started_at.elapsed().as_millis().min(u64::MAX as u128) as u64,
            provider,
        })
    }
}

/// 规范化供应商文本字段、协议和 BaseURL。
fn normalize_provider_input(input: &mut ProviderInput) -> Result<(), CommandError> {
    ProviderProtocol::parse(input.protocol.trim())?;
    input.name = input.name.trim().to_string();
    input.short_code = input.short_code.trim().to_string();
    input.protocol = input.protocol.trim().to_string();
    input.model = input.model.trim().to_string();
    input.base_url = normalize_base_url(&input.base_url)?;
    if input.name.is_empty() || input.short_code.is_empty() || input.model.is_empty() {
        return Err(CommandError::validation("供应商名称、简称和模型不能为空"));
    }
    Ok(())
}

/// 取出本次新密钥，并限制异常大的敏感输入。
fn take_new_api_key(input: &mut ProviderInput) -> Result<Option<String>, CommandError> {
    let api_key = input
        .api_key
        .take()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty());
    if api_key.as_ref().is_some_and(|value| value.len() > 16_384) {
        return Err(CommandError::validation("API Key 长度超过限制"));
    }
    Ok(api_key)
}

/// 限制内置订阅子类型只能使用 OAuth 和固定端点。
fn validate_provider_subtype(
    current: Option<&ProviderConfig>,
    provider_id: &str,
    input: &ProviderInput,
    new_api_key: Option<&str>,
) -> Result<(), CommandError> {
    let is_subscription = provider_id == OPENAI_SUBSCRIPTION_PROVIDER_ID
        || current.is_some_and(|config| config.provider_type == OPENAI_SUBSCRIPTION_PROVIDER_TYPE);
    if !is_subscription {
        return Ok(());
    }
    if new_api_key.is_some() {
        return Err(CommandError::validation(
            "OpenAI 订阅供应商只能使用 ChatGPT 登录，不能保存 API Key",
        ));
    }
    if input.protocol != "OpenAI Compatible"
        || input.base_url.trim_end_matches('/') != OPENAI_SUBSCRIPTION_ENDPOINT
    {
        return Err(CommandError::validation(
            "OpenAI 订阅供应商必须使用固定 Codex Responses 端点",
        ));
    }
    Ok(())
}

/// 防止旧密钥被协议变更或跨来源地址复用。
fn ensure_safe_secret_reuse(
    current: Option<&ProviderConfig>,
    input: &ProviderInput,
    new_api_key: Option<&str>,
) -> Result<(), CommandError> {
    let Some(current) = current.filter(|config| config.secret_ref.is_some()) else {
        return Ok(());
    };
    if new_api_key.is_none()
        && (current.protocol != input.protocol || !same_origin(&current.base_url, &input.base_url)?)
    {
        return Err(CommandError::validation(
            "修改协议或 BaseURL 来源时必须输入新 API Key，或重新登录 ChatGPT",
        ));
    }
    Ok(())
}

/// 比较两个供应商地址是否属于同一网络来源。
fn same_origin(left: &str, right: &str) -> Result<bool, CommandError> {
    let left = Url::parse(left).map_err(|_| CommandError::validation("原供应商地址无效"))?;
    let right = Url::parse(right).map_err(|_| CommandError::validation("新供应商地址无效"))?;
    Ok(left.origin().ascii_serialization() == right.origin().ascii_serialization())
}

/// 从前端表单和已保存摘要构造不含密钥的模型配置。
fn provider_config_from_input(
    input: &ProviderInput,
    current: Option<&ProviderConfig>,
) -> ProviderConfig {
    ProviderConfig {
        id: input.id.clone().unwrap_or_else(|| "unsaved".to_string()),
        protocol: input.protocol.clone(),
        model: input.model.clone(),
        base_url: input.base_url.clone(),
        secret_ref: None,
        auth_type: None,
        oauth_account_id: None,
        provider_type: current
            .map(|config| config.provider_type.clone())
            .unwrap_or_else(|| "api".to_string()),
        supports_vision: input.supports_vision,
    }
}

/// 判断连接测试是否完全对应已保存配置。
fn same_model_config(left: &ProviderConfig, right: &ProviderConfig) -> bool {
    left.id == right.id
        && left.protocol == right.protocol
        && left.model == right.model
        && left.base_url == right.base_url
        && left.supports_vision == right.supports_vision
}

/// 创建未提供可用密钥的错误。
fn missing_credential() -> CommandError {
    CommandError::new(
        "PROVIDER_CREDENTIAL_MISSING",
        "请先输入或保存 API Key，或登录 ChatGPT",
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 创建供应商安全规则测试使用的输入。
    fn test_input(base_url: &str) -> ProviderInput {
        ProviderInput {
            id: Some("provider".to_string()),
            name: "Provider".to_string(),
            short_code: "PR".to_string(),
            protocol: "OpenAI Compatible".to_string(),
            model: "model".to_string(),
            base_url: base_url.to_string(),
            supports_vision: false,
            api_key: None,
        }
    }

    #[test]
    /// 更换网络来源时不能静默复用旧密钥。
    fn requires_key_for_origin_change() {
        let current = ProviderConfig {
            id: "provider".to_string(),
            protocol: "OpenAI Compatible".to_string(),
            model: "model".to_string(),
            base_url: "https://old.example/v1/".to_string(),
            secret_ref: Some("ref".to_string()),
            auth_type: Some("api_key".to_string()),
            oauth_account_id: None,
            provider_type: "api".to_string(),
            supports_vision: false,
        };
        assert!(ensure_safe_secret_reuse(
            Some(&current),
            &test_input("https://new.example/v1/"),
            None
        )
        .is_err());
    }

    #[test]
    /// 订阅供应商允许自定义模型 ID 和图片能力标记。
    fn subscription_accepts_custom_model_and_vision() {
        let mut input = test_input(OPENAI_SUBSCRIPTION_ENDPOINT);
        input.id = Some(OPENAI_SUBSCRIPTION_PROVIDER_ID.to_string());
        input.model = "gpt-5.6-sol-high".to_string();
        input.supports_vision = true;
        assert!(
            validate_provider_subtype(None, OPENAI_SUBSCRIPTION_PROVIDER_ID, &input, None).is_ok()
        );
    }

    #[tokio::test]
    /// 加密保险库可以保存超过 Windows 单条凭据限制的供应商值。
    async fn stores_large_credential_in_encrypted_vault() {
        let database = Database::connect_memory()
            .await
            .expect("创建测试数据库失败");
        let services = AppServices::new().expect("创建应用服务失败");
        services
            .initialize(&database)
            .await
            .expect("初始化保险库失败");
        let credential = format!("sk-{}", "x".repeat(5_000));
        let mut input = test_input("https://example.com/v1");
        input.api_key = Some(credential.clone());
        let provider = services
            .save_provider(&database, input)
            .await
            .expect("保存大凭据失败");
        let config = database
            .get_provider_config(&provider.id)
            .await
            .expect("查询供应商配置失败")
            .expect("供应商配置不存在");
        assert_eq!(
            services
                .load_api_key(&database, &config)
                .await
                .expect("读取大凭据失败"),
            credential
        );
    }
}
