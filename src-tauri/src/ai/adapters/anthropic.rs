//! Anthropic Messages 适配器：wire 请求构造与 SSE/JSON 到 chunk 的翻译。

use reqwest::{Client, Request};
use serde_json::{json, Value};

use crate::ai::llm::adapter::LlmAdapter;
use crate::ai::llm::chunk::StreamChunk;
use crate::ai::llm::failure::{FailureCode, LlmFailure};
use crate::ai::llm::request::{Credential, ModelRequest, ModelRoute, ToolSchema};
use crate::ai::llm::vocabulary::{ContentBlock, FinishReason, Message, Role, TokenUsage};
use crate::ai::{apply_client_identity, completion_endpoint, ProviderProtocol};
use crate::error::CommandError;

pub(crate) struct Anthropic;

impl LlmAdapter for Anthropic {
    fn protocol(&self) -> &'static str {
        "anthropic_messages"
    }

    fn build(
        &self,
        client: &Client,
        route: &ModelRoute,
        request: &ModelRequest,
        credential: &Credential,
    ) -> Result<Request, CommandError> {
        let endpoint = completion_endpoint(&route.base_url, ProviderProtocol::AnthropicMessages)?;
        let mut body = json!({
            "model": route.model,
            "system": anthropic_system(&request.system, &request.messages),
            "messages": anthropic_messages(&request.messages),
            "tools": request.tools.iter().map(anthropic_tool_json).collect::<Vec<_>>(),
            "max_tokens": route.max_tokens.unwrap_or(crate::models::DEFAULT_MAX_OUTPUT_TOKENS),
            "temperature": route.temperature,
            "stream": true
        });
        if route.temperature.is_none() {
            if let Some(object) = body.as_object_mut() {
                object.remove("temperature");
            }
        }
        if let Some(name) = &request.tool_choice {
            body["tool_choice"] = json!({
                "type": "tool",
                "name": name,
                "disable_parallel_tool_use": true
            });
        }
        apply_client_identity(
            client
                .post(endpoint)
                .header("x-api-key", credential.token())
                .header("anthropic-version", "2023-06-01")
                .header("Accept", "text/event-stream")
                .json(&body),
            &route.base_url,
            &route.session_id,
        )
        .build()
        .map_err(|_| CommandError::provider("PROVIDER_REQUEST_INVALID", "无法构造模型请求"))
    }

    fn translate(&self, event: &Value) -> Vec<StreamChunk> {
        anthropic_chunks(event)
    }

    fn translate_json(&self, body: &Value) -> Vec<StreamChunk> {
        anthropic_json_chunks(body)
    }

    /// Anthropic 只有 message_stop 表示流完整收束。
    fn is_terminal(&self, event: &Value) -> bool {
        event.get("type").and_then(Value::as_str) == Some("message_stop")
    }
}

/// 顶层 system 消息与中立 System 消息合并，保持单一 system 字段。
fn anthropic_system(system: &str, messages: &[Message]) -> String {
    let mut parts = vec![system.to_string()];
    for message in messages {
        if message.role == Role::System {
            let text = message.text();
            if !text.is_empty() {
                parts.push(text);
            }
        }
    }
    parts.join("\n\n")
}

/// 中立消息转 Anthropic 消息；工具结果与文本块处于同一 user content 数组。
fn anthropic_messages(messages: &[Message]) -> Vec<Value> {
    let mut out = Vec::new();
    for message in messages {
        if message.role == Role::System {
            continue;
        }
        let role = match message.role {
            Role::Assistant => "assistant",
            _ => "user",
        };
        let mut content = Vec::new();
        for block in &message.blocks {
            match block {
                ContentBlock::Text { text } => {
                    if !text.is_empty() {
                        content.push(json!({"type": "text", "text": text}));
                    }
                }
                ContentBlock::ToolCall {
                    id,
                    name,
                    arguments,
                } => {
                    let input: Value =
                        serde_json::from_str(arguments).unwrap_or_else(|_| json!({}));
                    content
                        .push(json!({"type": "tool_use", "id": id, "name": name, "input": input}));
                }
                ContentBlock::ToolResult {
                    call_id, blocks, ..
                } => {
                    content.push(json!({
                        "type": "tool_result",
                        "tool_use_id": call_id,
                        "content": tool_result_content(blocks)
                    }));
                }
                ContentBlock::Image {
                    mime, data_base64, ..
                } => {
                    content.push(json!({
                        "type": "image",
                        "source": {"type": "base64", "media_type": mime, "data": data_base64}
                    }));
                }
                ContentBlock::Reasoning { .. } => {}
            }
        }
        if content.is_empty() {
            content.push(json!({"type": "text", "text": "（无文字）"}));
        }
        out.push(json!({"role": role, "content": content}));
    }
    out
}

/// 工具结果默认是纯文本字符串；含图片时使用 text/image 块数组，Anthropic 才允许回图。
fn tool_result_content(blocks: &[ContentBlock]) -> Value {
    let images: Vec<Value> = blocks
        .iter()
        .filter_map(|block| match block {
            ContentBlock::Image {
                mime, data_base64, ..
            } => Some(json!({
                "type": "image",
                "source": {"type": "base64", "media_type": mime, "data": data_base64}
            })),
            _ => None,
        })
        .collect();
    if images.is_empty() {
        return Value::String(blocks_text(blocks));
    }
    let mut content = vec![json!({"type": "text", "text": blocks_text(blocks)})];
    content.extend(images);
    Value::Array(content)
}

/// 只拼接可见文本块。
fn blocks_text(blocks: &[ContentBlock]) -> String {
    blocks
        .iter()
        .filter_map(|block| match block {
            ContentBlock::Text { text } => Some(text.as_str()),
            _ => None,
        })
        .collect()
}

/// Anthropic 工具定义使用 input_schema。
fn anthropic_tool_json(tool: &ToolSchema) -> Value {
    json!({
        "name": tool.name,
        "description": tool.description,
        "input_schema": tool.parameters
    })
}

/// SSE 事件到 chunk；文本/思考用 content block index 关联。
fn anthropic_chunks(event: &Value) -> Vec<StreamChunk> {
    let mut chunks = Vec::new();
    let kind = event
        .get("type")
        .and_then(Value::as_str)
        .unwrap_or_default();
    match kind {
        "content_block_start" => {
            if event.pointer("/content_block/type").and_then(Value::as_str) == Some("tool_use") {
                let index = event.get("index").and_then(Value::as_u64).unwrap_or(0) as usize;
                chunks.push(StreamChunk::ToolCallDelta {
                    index,
                    id: event
                        .pointer("/content_block/id")
                        .and_then(Value::as_str)
                        .unwrap_or_default()
                        .into(),
                    item_id: None,
                    name: event
                        .pointer("/content_block/name")
                        .and_then(Value::as_str)
                        .map(str::to_string),
                    arguments_delta: String::new(),
                });
            }
        }
        "content_block_delta" => {
            let index = event.get("index").and_then(Value::as_u64).unwrap_or(0) as usize;
            let delta = &event["delta"];
            if delta.get("type").and_then(Value::as_str) == Some("input_json_delta") {
                if let Some(partial) = delta.get("partial_json").and_then(Value::as_str) {
                    chunks.push(StreamChunk::ToolCallDelta {
                        index,
                        id: String::new(),
                        item_id: None,
                        name: None,
                        arguments_delta: partial.into(),
                    });
                }
            } else {
                if let Some(thinking) = delta.get("thinking").and_then(Value::as_str) {
                    if !thinking.is_empty() {
                        chunks.push(StreamChunk::ReasoningDelta {
                            index,
                            text: thinking.into(),
                        });
                    }
                }
                if let Some(text) = delta.get("text").and_then(Value::as_str) {
                    if !text.is_empty() {
                        chunks.push(StreamChunk::TextDelta {
                            index,
                            text: text.into(),
                        });
                    }
                }
            }
        }
        "message_start" => {
            if let Some(usage) = event
                .pointer("/message/usage")
                .and_then(parse_anthropic_usage)
            {
                chunks.push(StreamChunk::Usage(usage));
            }
        }
        "message_delta" => {
            if let Some(usage) = event.get("usage").and_then(parse_anthropic_usage) {
                chunks.push(StreamChunk::Usage(usage));
            }
            if let Some(reason) = event.pointer("/delta/stop_reason").and_then(Value::as_str) {
                chunks.push(StreamChunk::Finish {
                    reason: anthropic_finish_reason(reason),
                    failure: None,
                    replay: None,
                });
            }
        }
        "error" => {
            chunks.push(StreamChunk::Finish {
                reason: FinishReason::Error,
                failure: Some(anthropic_failure(event)),
                replay: None,
            });
        }
        _ => {}
    }
    chunks
}

/// 非流式响应整体翻译；无工具即正常结束。
fn anthropic_json_chunks(body: &Value) -> Vec<StreamChunk> {
    let mut chunks = Vec::new();
    if let Some(usage) = body.get("usage").and_then(parse_anthropic_usage) {
        chunks.push(StreamChunk::Usage(usage));
    }
    let Some(blocks) = body.get("content").and_then(Value::as_array) else {
        chunks.push(protocol_error_chunk());
        return chunks;
    };
    let mut has_calls = false;
    for (index, block) in blocks.iter().enumerate() {
        match block.get("type").and_then(Value::as_str) {
            Some("text") => {
                if let Some(text) = block.get("text").and_then(Value::as_str) {
                    if !text.is_empty() {
                        chunks.push(StreamChunk::TextDelta {
                            index,
                            text: text.into(),
                        });
                    }
                }
            }
            Some("tool_use") => {
                has_calls = true;
                chunks.push(StreamChunk::ToolCallDelta {
                    index,
                    id: block
                        .get("id")
                        .and_then(Value::as_str)
                        .unwrap_or_default()
                        .into(),
                    item_id: None,
                    name: block
                        .get("name")
                        .and_then(Value::as_str)
                        .map(str::to_string),
                    arguments_delta: block
                        .get("input")
                        .cloned()
                        .unwrap_or(Value::Null)
                        .to_string(),
                });
            }
            _ => {}
        }
    }
    chunks.push(StreamChunk::Finish {
        reason: if has_calls {
            FinishReason::ToolCalls
        } else {
            FinishReason::Stop
        },
        failure: None,
        replay: None,
    });
    chunks
}

/// in-band 错误归类；overloaded_error 可重试。
fn anthropic_failure(event: &Value) -> LlmFailure {
    let kind = event.pointer("/error/type").and_then(Value::as_str);
    if kind == Some("overloaded_error") {
        LlmFailure::provider(FailureCode::Overloaded, "模型服务当前负载过高，请稍后重试")
    } else {
        LlmFailure::provider(FailureCode::Transport, "供应商流返回失败事件，请稍后重试")
    }
}

/// 结构无法识别时的安全终止。
fn protocol_error_chunk() -> StreamChunk {
    StreamChunk::Finish {
        reason: FinishReason::Error,
        failure: Some(LlmFailure::provider(
            FailureCode::ResponseInvalid,
            "供应商返回了无法识别的响应",
        )),
        replay: None,
    }
}

/// 结束原因白名单映射。
fn anthropic_finish_reason(reason: &str) -> FinishReason {
    match reason {
        "max_tokens" => FinishReason::MaxTokens,
        "tool_use" => FinishReason::ToolCalls,
        _ => FinishReason::Stop,
    }
}

/// Anthropic 用量字段为 input_tokens/output_tokens。
fn parse_anthropic_usage(value: &Value) -> Option<TokenUsage> {
    let input = value.get("input_tokens").and_then(Value::as_u64);
    let output = value.get("output_tokens").and_then(Value::as_u64);
    if input.is_none() && output.is_none() {
        return None;
    }
    Some(TokenUsage {
        input_tokens: input.unwrap_or(0),
        output_tokens: output.unwrap_or(0),
        ..Default::default()
    })
}

#[cfg(test)]
#[path = "anthropic_tests.rs"]
mod tests;
