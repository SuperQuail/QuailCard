//! providers.toml 的文件结构定义与内置供应商播种。

use serde::{Deserialize, Serialize};

use super::{envelope, now_timestamp};

/// 供应商配置文件名，位于应用配置目录。
pub(crate) const FILE_NAME: &str = "providers.toml";

/// providers.toml 的完整结构。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub(crate) struct ProvidersFile {
    /// 信封版本，缺失按当前版本解析。
    pub(crate) format_version: u64,
    /// 当前活动供应商标识。
    pub(crate) active_provider_id: Option<String>,
    /// 全部供应商记录。
    pub(crate) providers: Vec<ProviderRecord>,
}

impl Default for ProvidersFile {
    /// 缺文件时由播种函数填充内置供应商，这里只提供空骨架。
    fn default() -> Self {
        Self {
            format_version: envelope::CURRENT_FORMAT_VERSION,
            active_provider_id: None,
            providers: Vec::new(),
        }
    }
}

/// 单个供应商的持久化记录；除 id 外全部字段都有默认值以支持容错读取。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub(crate) struct ProviderRecord {
    /// 供应商稳定标识，内置供应商使用固定 id。
    pub(crate) id: String,
    pub(crate) name: String,
    pub(crate) short_code: String,
    pub(crate) protocol: String,
    pub(crate) model: String,
    pub(crate) base_url: String,
    /// 是否以 API Key 形式持有凭据（派生自 authType）。
    pub(crate) has_api_key: bool,
    pub(crate) supports_vision: bool,
    /// 连接测试状态：connected / untested。
    pub(crate) status: String,
    /// 加密保险库中的凭据随机引用。
    pub(crate) secret_ref: Option<String>,
    /// 认证类型：api_key / openai_oauth / None。
    pub(crate) auth_type: Option<String>,
    /// OAuth 账号摘要（不含令牌）。
    pub(crate) oauth_account_id: Option<String>,
    /// 供应商子类型：api / openai_subscription。
    pub(crate) provider_type: String,
    pub(crate) created_at: i64,
    pub(crate) updated_at: i64,
}

impl Default for ProviderRecord {
    /// 容错读取的默认值：空配置、未测试的普通 API 供应商。
    fn default() -> Self {
        Self {
            id: String::new(),
            name: String::new(),
            short_code: String::new(),
            protocol: String::new(),
            model: String::new(),
            base_url: String::new(),
            has_api_key: false,
            supports_vision: false,
            status: "untested".to_string(),
            secret_ref: None,
            auth_type: None,
            oauth_account_id: None,
            provider_type: "api".to_string(),
            created_at: 0,
            updated_at: 0,
        }
    }
}

/// 首次启动时播种内置供应商与默认活动供应商。
pub(crate) fn seed_providers_file() -> ProvidersFile {
    let now = now_timestamp();
    let builtin = |id: &str,
                   name: &str,
                   short_code: &str,
                   protocol: &str,
                   model: &str,
                   base_url: &str,
                   supports_vision: bool,
                   provider_type: &str| ProviderRecord {
        id: id.to_string(),
        name: name.to_string(),
        short_code: short_code.to_string(),
        protocol: protocol.to_string(),
        model: model.to_string(),
        base_url: base_url.to_string(),
        has_api_key: false,
        supports_vision,
        status: "untested".to_string(),
        secret_ref: None,
        auth_type: None,
        oauth_account_id: None,
        provider_type: provider_type.to_string(),
        created_at: now,
        updated_at: now,
    };
    ProvidersFile {
        format_version: envelope::CURRENT_FORMAT_VERSION,
        active_provider_id: Some("openai".to_string()),
        providers: vec![
            builtin(
                "openai",
                "OpenAI",
                "OA",
                "OpenAI Compatible",
                "gpt-4.1-mini",
                "https://api.openai.com/v1",
                true,
                "api",
            ),
            builtin(
                "anthropic",
                "Anthropic",
                "AN",
                "Anthropic Messages",
                "claude-sonnet-4-5",
                "https://api.anthropic.com",
                true,
                "api",
            ),
            builtin(
                "openai_subscription",
                "OpenAI 订阅",
                "OS",
                "OpenAI Compatible",
                "gpt-5.5",
                "https://chatgpt.com/backend-api/codex/responses",
                true,
                "openai_subscription",
            ),
            builtin(
                "opencode_go",
                "OpenCode Go",
                "OG",
                "OpenAI Compatible",
                "deepseek-v4-flash",
                "https://opencode.ai/zen/go/v1",
                false,
                "api",
            ),
        ],
    }
}
