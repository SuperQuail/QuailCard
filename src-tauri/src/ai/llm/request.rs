//! 模型请求的中立描述：路由、凭据、请求与结果，不含供应商 wire 细节。

use serde_json::Value;

use super::chunk::ReplayEnvelope;
use super::failure::LlmFailure;
use super::vocabulary::{ContentBlock, FinishReason, Message, TokenUsage};
use crate::ai::{ProviderProtocol, ToolCallResult};

/// 已校验的模型路由；组合根解析一次，运行期不再对协议做 match。
#[derive(Debug, Clone)]
pub(crate) struct ModelRoute {
    pub provider_id: String,
    pub protocol: ProviderProtocol,
    /// 认证方式固定标签：api_key | openai_oauth。
    pub auth_type: &'static str,
    pub model: String,
    pub base_url: String,
    pub max_tokens: Option<u32>,
    pub temperature: Option<f32>,
    pub parallel_tool_calls: bool,
    pub idle_timeout_ms: u64,
    /// 对话身份；OpenCode 会话头等供应商归属信息由此派生。
    pub session_id: String,
}

/// 请求级凭据；只覆盖一次调用，离开作用域即由 Zeroizing 清零。
pub(crate) enum Credential {
    ApiKey(zeroize::Zeroizing<String>),
    OAuth {
        access_token: zeroize::Zeroizing<String>,
        account_id: Option<String>,
    },
}

impl Credential {
    /// 只读暴露 token；凭据不得进入日志、错误消息或前端 DTO。
    pub(crate) fn token(&self) -> &str {
        match self {
            Self::ApiKey(token) => token,
            Self::OAuth { access_token, .. } => access_token,
        }
    }

    /// OAuth 账号路由头取值。
    pub(crate) fn account_id(&self) -> Option<&str> {
        match self {
            Self::OAuth { account_id, .. } => account_id.as_deref(),
            Self::ApiKey(_) => None,
        }
    }

    /// 是否使用 ChatGPT OAuth 凭据。
    pub(crate) fn is_oauth(&self) -> bool {
        matches!(self, Self::OAuth { .. })
    }
}

/// 模型可见的工具线格式；名字与描述拥有所有权，可承载动态工具集。
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct ToolSchema {
    pub name: String,
    pub description: String,
    pub parameters: Value,
}

/// 一次模型请求的中立输入。
pub(crate) struct ModelRequest {
    pub system: String,
    pub messages: Vec<Message>,
    pub tools: Vec<ToolSchema>,
    /// 强制调用某个工具；None 表示不设置 tool_choice。
    pub tool_choice: Option<String>,
}

/// 一次模型调用的完整结果；工具参数保留原始诊断。
#[derive(Debug, Clone)]
pub(crate) struct AssistantTurn {
    pub text: String,
    pub blocks: Vec<ContentBlock>,
    pub calls: Vec<ToolCallResult>,
    pub usage: Option<TokenUsage>,
    pub finish: FinishReason,
    pub failure: Option<LlmFailure>,
    pub replay: Option<ReplayEnvelope>,
}
