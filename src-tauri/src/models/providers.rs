//! 供应商与保险库的 wire 契约：前端 DTO 与后端模型请求配置的唯一来源。
//!
//! 该分组从 `models.rs` 拆出，避免单文件超过 500 行上限；所有类型仍由
//! `models.rs` 的 `pub use providers::*;` 重导出，外部引用路径保持不变。
//! 字段统一 camelCase；演化遵守"读容忍、写完整、只增不改"（见 storage 模块文档）。

use serde::{Deserialize, Serialize};

/// 供应商没有配置输出上限时使用的兜底值：32K token。
pub const DEFAULT_MAX_OUTPUT_TOKENS: u32 = 32_768;
/// 单个供应商允许的模型目录条目上限，防止前端提交异常大的数组。
pub const MAX_PROVIDER_MODELS: usize = 32;
/// 上下文窗口的最小合法值：低于该值的条目没有实际意义，视为填错。
pub const MIN_CONTEXT_WINDOW_TOKENS: u64 = 1024;
/// 单次最大输出 token 的合法上限，超过该值必定是误填。
pub const MAX_MODEL_OUTPUT_TOKENS: u32 = 200_000;

/// 供应商模型目录中的单个模型条目。
///
/// 契约：`id` 是真正写进模型请求的标识，`name` 只用于界面展示，为空时读取方
/// 回退为 `id`；两个上限字段都可选，缺失表示"未知/用默认"而非 0。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct ProviderModel {
    /// 请求使用的模型 id；同一供应商内必须非空且唯一。
    pub id: String,
    /// 界面显示名称；为空时由读取方回退为 id。
    pub name: String,
    /// 上下文窗口 token 数；None 表示未知，仅用于展示与预算参考。
    pub context_window: Option<u64>,
    /// 单次最大输出 token；None 或 0 表示回退 DEFAULT_MAX_OUTPUT_TOKENS。
    pub max_output_tokens: Option<u32>,
}

impl Default for ProviderModel {
    /// 支持 `#[serde(default)]` 的缺省读取：空条目，全部上限未配置。
    fn default() -> Self {
        Self {
            id: String::new(),
            name: String::new(),
            context_window: None,
            max_output_tokens: None,
        }
    }
}

impl ProviderModel {
    /// 规范化为可持久化、可下发的条目：去空白并在 name 缺省时回退 id。
    ///
    /// 契约：id 为空表示条目无效，由调用方丢弃；name 永远不写出空串。
    pub fn normalized(&self) -> Self {
        let id = self.id.trim().to_string();
        let name = self.name.trim();
        Self {
            name: if name.is_empty() {
                id.clone()
            } else {
                name.to_string()
            },
            id,
            context_window: self.context_window,
            max_output_tokens: self.max_output_tokens,
        }
    }
}

/// 模型供应商的非敏感摘要。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderSummary {
    pub id: String,
    pub name: String,
    pub short_code: String,
    pub protocol: String,
    /// 当前使用模型 id；与 models 中选中条目一致，读旧字段的代码继续可用。
    pub model: String,
    /// 模型目录；序列化始终输出，旧文件（只有 model）读出时合成单条。
    pub models: Vec<ProviderModel>,
    pub base_url: String,
    pub has_api_key: bool,
    pub has_credential: bool,
    pub auth_type: Option<String>,
    pub oauth_account_id: Option<String>,
    pub provider_type: String,
    pub supports_vision: bool,
    pub status: String,
}

/// 保存供应商非敏感配置的输入。
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderInput {
    pub id: Option<String>,
    pub name: String,
    pub short_code: String,
    pub protocol: String,
    /// 前端同步的当前使用模型 id；后端仍以它匹配 active_model。
    pub model: String,
    /// 模型目录；老前端不传时按空数组处理，后端回退到用 model 合成单条。
    #[serde(default)]
    pub models: Vec<ProviderModel>,
    pub base_url: String,
    pub supports_vision: bool,
    pub api_key: Option<String>,
}

/// 后端发起模型请求所需的供应商配置。
#[derive(Debug, Clone)]
pub struct ProviderConfig {
    pub id: String,
    pub protocol: String,
    /// 当前使用模型 id；active_model 按它匹配目录条目。
    pub model: String,
    /// 模型目录；单次请求的输出上限从选中条目读取。
    pub models: Vec<ProviderModel>,
    pub base_url: String,
    pub secret_ref: Option<String>,
    pub auth_type: Option<String>,
    pub oauth_account_id: Option<String>,
    pub provider_type: String,
    pub supports_vision: bool,
}

/// 加密保险库内保存的 OpenAI OAuth 凭据。
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OpenAiOAuthCredential {
    pub access_token: String,
    pub refresh_token: String,
    pub expires_at: i64,
    pub account_id: Option<String>,
}

impl Drop for OpenAiOAuthCredential {
    /// 包括取消请求和解析后提前返回的路径，都必须清零解密令牌。
    fn drop(&mut self) {
        use zeroize::Zeroize;
        self.access_token.zeroize();
        self.refresh_token.zeroize();
    }
}

/// OpenAI OAuth 登录方式。
#[derive(Debug, Clone, Copy, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OpenAiLoginMode {
    Browser,
    Device,
}

/// 启动 OpenAI 登录后返回给界面的信息。
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OpenAiLoginStart {
    pub attempt_id: String,
    pub mode: String,
    pub url: String,
    pub user_code: Option<String>,
}

/// 查询 OpenAI 登录进度的结果。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OpenAiLoginStatus {
    pub status: String,
    pub message: String,
    pub provider: Option<ProviderSummary>,
}

/// vault.bin 中保存的认证加密保险库密文记录。
///
/// 字节字段以 base64 编码持久化；结构变更遵循存储层"只增不改"契约。
#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VaultEnvelope {
    pub format_version: i64,
    pub protection_mode: String,
    #[serde(with = "crate::storage::envelope::base64_field")]
    pub kdf_salt: Vec<u8>,
    pub kdf_iterations: Option<i64>,
    #[serde(with = "crate::storage::envelope::base64_field")]
    pub nonce: Vec<u8>,
    #[serde(with = "crate::storage::envelope::base64_field")]
    pub ciphertext: Vec<u8>,
}

/// 前端可见的保险库保护状态。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VaultStatus {
    pub protection_mode: String,
    pub locked: bool,
}

/// 连接测试的耗时结果。
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ConnectionTestResult {
    pub latency_ms: u64,
    pub provider: Option<ProviderSummary>,
}

impl ProviderConfig {
    /// 当前使用的模型条目：先按 `model` 匹配 id，匹配不到时退回目录首项。
    ///
    /// 契约：`model` 始终是权威来源（前端把它同步为目录选中项的 id）；目录为
    /// 空（尚未回填目录的旧记录）时返回 None，由调用方回退默认输出上限。
    pub fn active_model(&self) -> Option<&ProviderModel> {
        let selected = self.model.trim();
        self.models
            .iter()
            .find(|entry| entry.id.trim() == selected)
            .or_else(|| self.models.first())
    }

    /// 本次模型请求可用的最大输出 token。
    ///
    /// 契约：取 active_model 的 max_output_tokens；目录缺失、条目未配置或
    /// 填 0 时回退 DEFAULT_MAX_OUTPUT_TOKENS，绝不退化成"无上限"请求。
    pub fn max_output_tokens(&self) -> u32 {
        self.active_model()
            .and_then(|entry| entry.max_output_tokens)
            .filter(|value| *value > 0)
            .unwrap_or(DEFAULT_MAX_OUTPUT_TOKENS)
    }
}
