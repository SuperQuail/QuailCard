//! 流式 chunk 协议：adapter 只产出这里的变体，调用方只消费这里的变体。

use serde_json::Value;

use super::failure::LlmFailure;
use super::vocabulary::{ContentBlock, FinishReason, TokenUsage};

/// 块类型标签；adapter 在 block-start 声明，后续 delta 用 index 关联。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ContentKind {
    Text,
    Reasoning,
    Image,
    ToolCall,
}

/// 成功响应的重放元数据；对调用方不透明，只在同一 adapter 内回放。
#[derive(Debug, Clone, Default, PartialEq)]
pub(crate) struct ReplayEnvelope {
    pub response: Option<Value>,
    pub blocks: Vec<Option<Value>>,
}

/// 平坦流协议。契约：Usage 必须在 Finish 前；Finish 之后不得再有 chunk。
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum StreamChunk {
    BlockStart {
        index: usize,
        kind: ContentKind,
    },
    TextDelta {
        index: usize,
        text: String,
    },
    ReasoningDelta {
        index: usize,
        text: String,
    },
    ToolCallDelta {
        index: usize,
        id: String,
        /// Responses 无状态续传所需的输出项身份；其他协议为 None。
        item_id: Option<String>,
        name: Option<String>,
        arguments_delta: String,
    },
    BlockEnd {
        index: usize,
        block: ContentBlock,
    },
    Usage(TokenUsage),
    Finish {
        reason: FinishReason,
        failure: Option<LlmFailure>,
        replay: Option<ReplayEnvelope>,
    },
}
