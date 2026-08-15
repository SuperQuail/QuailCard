mod client;
mod evaluation;
mod generation;
mod request;
mod response;
mod stream;

pub(crate) use client::normalize_base_url;
pub use client::AiClient;
pub use evaluation::{build_evaluation_prompt, evaluation_tool, parse_evaluation_response};
pub use generation::{build_generation_prompt, generation_tools, parse_generation_response};
pub(crate) use stream::debug_stage;

use crate::{error::CommandError, models::GenerationImage};
use serde_json::Value;
use std::time::Duration;

/// 强制模型调用的单个结构化输出工具。
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
    pub arguments: Value,
}

/// 一轮模型响应中的工具调用及供应商专用续传项。
#[derive(Debug)]
pub struct ToolCallBatch {
    pub calls: Vec<ToolCallResult>,
    pub continuation_items: Vec<Value>,
}

/// 多轮工具调用中的协议无关消息，用于构造请求历史。
#[derive(Debug, Clone)]
pub enum ToolMessage {
    /// 模型发起的工具调用，等待工具执行结果。
    AssistantCall {
        id: String,
        item_id: Option<String>,
        name: String,
        arguments: Value,
    },
    /// 工具执行完成后返回给模型的结果文本。
    ToolResult { id: String, content: String },
    /// Responses 无状态续传所需的原始输出项，包括加密 reasoning 内容。
    ProviderItem { value: Value },
}

/// 单次强制工具调用的提示词和请求限制。
pub struct ToolRequest<'a> {
    pub trace_id: &'a str,
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
