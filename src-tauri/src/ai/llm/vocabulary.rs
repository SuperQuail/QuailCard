//! 协议无关的会话词汇：DSH Message / ContentBlock 的 Rust 对应物。
//! 供应商 wire 格式只在 adapter 内翻译，本文件不出现任何供应商字段。

use serde::{Deserialize, Serialize};

/// 会话角色。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub(crate) enum Role {
    System,
    User,
    Assistant,
}

/// 工具调用身份统一为字符串，不区分供应商前缀。
pub(crate) type ToolCallId = String;

/// 消息来源只记录稳定标签，供投影与调试，不携带用户配置字符串。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum MessageSource {
    User,
    Model,
    Tool,
    Context,
}

/// 一条会话消息；id 在持久化与投影之间保持稳定。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub(crate) struct Message {
    pub id: String,
    pub role: Role,
    pub blocks: Vec<ContentBlock>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source: Option<MessageSource>,
    /// adapter 私有重放状态；只在同一 adapter 内解释，不做跨协议转换。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub replay: Option<serde_json::Value>,
}

impl Message {
    /// 构造一条用户文本消息。
    pub(crate) fn user_text(id: impl Into<String>, text: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            role: Role::User,
            blocks: vec![ContentBlock::Text { text: text.into() }],
            source: Some(MessageSource::User),
            replay: None,
        }
    }

    /// 只拼接可见文本块；思考与工具参数不属于模型可见答案。
    pub(crate) fn text(&self) -> String {
        self.blocks
            .iter()
            .filter_map(|block| match block {
                ContentBlock::Text { text } => Some(text.as_str()),
                _ => None,
            })
            .collect()
    }
}

/// 内容块联合；新增块类型必须同时更新 assembler 与 adapter。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "kebab-case")]
pub(crate) enum ContentBlock {
    Text {
        text: String,
    },
    Reasoning {
        text: String,
    },
    Image {
        name: String,
        mime: String,
        data_base64: String,
    },
    /// 工具参数保持原始 JSON 字符串，禁止在组装阶段提前解析。
    ToolCall {
        id: ToolCallId,
        name: String,
        arguments: String,
    },
    ToolResult {
        call_id: ToolCallId,
        blocks: Vec<ContentBlock>,
        is_error: bool,
    },
}

/// 终止原因；Aborted / Error 的具体事实由伴随的失败描述。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum FinishReason {
    Stop,
    ToolCalls,
    MaxTokens,
    Aborted,
    Error,
}

/// 令牌用量；口径与供应商无关，缺省字段表示供应商未提供。
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub(crate) struct TokenUsage {
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub cache_read_tokens: Option<u64>,
    pub cache_write_tokens: Option<u64>,
    pub reasoning_tokens: Option<u64>,
    pub total_tokens: Option<u64>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    /// 可见文本只拼接 Text 块，思考与工具参数不进入。
    fn text_skips_reasoning_and_tools() {
        let message = Message {
            id: "m1".into(),
            role: Role::Assistant,
            blocks: vec![
                ContentBlock::Reasoning {
                    text: "思考".into(),
                },
                ContentBlock::Text {
                    text: "答案".into(),
                },
                ContentBlock::ToolCall {
                    id: "c1".into(),
                    name: "emit_card".into(),
                    arguments: "{}".into(),
                },
            ],
            source: Some(MessageSource::Model),
            replay: None,
        };
        assert_eq!(message.text(), "答案");
    }

    #[test]
    /// 工具参数序列化后仍是原始 JSON 字符串，不被解析成对象。
    fn tool_arguments_stay_raw_string() {
        let block = ContentBlock::ToolCall {
            id: "c1".into(),
            name: "emit_card".into(),
            arguments: "{\"schema_version\":1}".into(),
        };
        let json = serde_json::to_value(&block).unwrap();
        assert!(json["arguments"].is_string());
        assert_eq!(json["type"], "tool-call");
    }
}
