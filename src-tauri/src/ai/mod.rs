pub(crate) mod adapters;
pub(crate) mod agent;
mod bridge;
mod evaluation;
mod gateway;
mod generation;
pub(crate) mod llm;
pub(crate) mod tools;

pub(crate) use gateway::{completion_endpoint, normalize_base_url, ProviderGateway};

/// 组合根：唯一一处把内置 adapter 注册进运行时，Agent 与非 Agent 入口共用。
pub(crate) fn configured_runtime() -> Result<llm::runtime::LlmRuntime, CommandError> {
    let runtime = llm::runtime::LlmRuntime::new()?;
    adapters::register_builtin(runtime.adapters())?;
    Ok(runtime)
}
pub use evaluation::{build_evaluation_prompt, evaluation_tool, parse_evaluation_response};
pub use generation::{
    build_generation_prompt, generation_mode_prompt, generation_tools, validate_generation_input,
    GenerationSession,
};
pub(crate) use llm::identity::{apply_client_identity, USER_AGENT};
/// 在 Debug 构建中输出不含载荷的有界阶段日志。
pub(crate) fn debug_stage(trace_id: &str, message: impl AsRef<str>) {
    #[cfg(debug_assertions)]
    {
        eprintln!("[QuailCard][AI trace={trace_id}] {}", message.as_ref());
    }
    #[cfg(not(debug_assertions))]
    let _ = (trace_id, message);
}

use crate::{error::CommandError, models::GenerationImage};
use serde_json::Value;
use std::time::Duration;

/// 强制模型调用的单个结构化输出工具。
#[derive(Debug, Clone)]
pub struct ToolDefinition {
    pub name: &'static str,
    pub description: &'static str,
    pub input_schema: Value,
}

/// 模型在一次响应中发起的工具调用。
#[derive(Debug, Clone)]
pub struct ToolCallResult {
    pub id: String,
    pub item_id: Option<String>,
    pub name: String,
    pub arguments: ToolArguments,
}

/// 工具参数 JSON 的安全解析状态，不保留无效原文。
#[derive(Debug, Clone, serde::Serialize)]
pub enum ToolArguments {
    /// 参数是完整有效的 JSON 值。
    Valid(Value),
    /// 参数不是有效 JSON，仅保留定位和分类信息。
    Invalid(ToolArgumentError),
}

impl ToolArguments {
    /// 从原始 JSON 字符串解析；失败只保留定位与分类，不保留原文。
    pub(crate) fn parse(arguments: &str) -> Self {
        let arguments = if arguments.trim().is_empty() {
            "{}"
        } else {
            arguments
        };
        match serde_json::from_str(arguments) {
            Ok(value) => Self::Valid(value),
            Err(error) => Self::Invalid(ToolArgumentError {
                line: error.line(),
                column: error.column(),
                category: match error.classify() {
                    serde_json::error::Category::Io => "io",
                    serde_json::error::Category::Syntax => "syntax",
                    serde_json::error::Category::Data => "data",
                    serde_json::error::Category::Eof => "eof",
                },
            }),
        }
    }

    /// 返回有效参数，供严格单工具消费者拒绝无效调用。
    pub fn valid(&self) -> Option<&Value> {
        match self {
            Self::Valid(value) => Some(value),
            Self::Invalid(_) => None,
        }
    }

    /// 返回可安全跨协议回放的参数，无效原文统一替换为空对象。
    pub fn replay_value(&self) -> Value {
        self.valid()
            .cloned()
            .unwrap_or_else(|| serde_json::json!({}))
    }
}

/// 无效工具参数的安全诊断，不包含模型返回的原始载荷。
#[derive(Debug, Clone, serde::Serialize)]
pub struct ToolArgumentError {
    pub line: usize,
    pub column: usize,
    pub category: &'static str,
}

/// 一轮模型响应中的工具调用及供应商专用续传项。
#[derive(Debug)]
pub struct ToolCallBatch {
    pub calls: Vec<ToolCallResult>,
    pub continuation_items: Vec<Value>,
}

/// 多轮工具调用中的协议无关消息，用于构造请求历史。
#[derive(Debug, Clone, serde::Serialize)]
pub enum ToolMessage {
    /// 模型发起的工具调用，等待工具执行结果。
    AssistantCall {
        id: String,
        item_id: Option<String>,
        name: String,
        arguments: ToolArguments,
    },
    /// 工具执行完成后返回给模型的结果文本。
    ToolResult { id: String, content: String },
    /// Responses 无状态续传所需的原始输出项，包括加密 reasoning 内容。
    ProviderItem { value: Value },
}

/// 单次强制工具调用的提示词和请求限制。
pub struct ToolRequest<'a> {
    pub trace_id: &'a str,
    /// 轮次保留在请求契约中，供日志与后续循环使用。
    #[allow(dead_code)]
    pub turn: usize,
    pub system_prompt: &'a str,
    pub user_prompt: &'a str,
    pub images: &'a [GenerationImage],
    pub tool: &'a ToolDefinition,
    pub max_tokens: u32,
    pub timeout: Duration,
}

/// 允许模型自主选择工具的多工具请求，携带此前轮次的调用历史。
pub struct MultiToolRequest<'a> {
    pub trace_id: &'a str,
    /// 轮次保留在请求契约中，供日志与后续循环使用。
    #[allow(dead_code)]
    pub turn: usize,
    pub system_prompt: &'a str,
    pub user_prompt: &'a str,
    pub images: &'a [GenerationImage],
    pub tools: &'a [ToolDefinition],
    pub history: &'a [ToolMessage],
    pub max_tokens: u32,
    pub timeout: Duration,
}

/// 后端支持的模型请求协议。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProviderProtocol {
    OpenAiCompatible,
    AnthropicMessages,
}

impl ProviderProtocol {
    /// 将数据库中的协议名称解析为稳定枚举。
    pub fn parse(value: &str) -> Result<Self, CommandError> {
        match value {
            "OpenAI Compatible" => Ok(Self::OpenAiCompatible),
            "Anthropic Messages" => Ok(Self::AnthropicMessages),
            _ => Err(CommandError::validation("不支持的供应商协议")),
        }
    }
}
