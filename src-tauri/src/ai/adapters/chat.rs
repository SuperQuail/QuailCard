//! OpenAI Compatible Chat 适配器：wire 请求构造与 SSE/JSON 到 chunk 的翻译。

use reqwest::{Client, Request};
use serde_json::{json, Value};

use crate::ai::llm::adapter::LlmAdapter;
use crate::ai::llm::chunk::StreamChunk;
use crate::ai::llm::failure::{FailureCode, LlmFailure};
use crate::ai::llm::request::{Credential, ModelRequest, ModelRoute, ToolSchema};
use crate::ai::llm::vocabulary::{ContentBlock, FinishReason, Message, Role, TokenUsage};
use crate::ai::{apply_client_identity, completion_endpoint, ProviderProtocol};
use crate::error::CommandError;

/// 思考固定 0 号块，文本 1 号块；工具调用从 2 号起，避免交错时索引冲突。
const REASONING_INDEX: usize = 0;
const TEXT_INDEX: usize = 1;
const TOOL_INDEX_BASE: usize = 2;

pub(crate) struct Chat;

impl LlmAdapter for Chat {
    /// 固定协议标识用于隔离适配器私有状态。
    fn protocol(&self) -> &'static str {
        "openai_chat"
    }

    /// 按 Chat 契约构建认证请求，不向供应商透传中立消息结构。
    fn build(
        &self,
        client: &Client,
        route: &ModelRoute,
        request: &ModelRequest,
        credential: &Credential,
    ) -> Result<Request, CommandError> {
        let endpoint = completion_endpoint(&route.base_url, ProviderProtocol::OpenAiCompatible)?;
        let mut body = json!({
            "model": route.model,
            "messages": chat_messages(&request.system, &request.messages),
            "tools": request.tools.iter().map(openai_tool_json).collect::<Vec<_>>(),
            "parallel_tool_calls": route.parallel_tool_calls,
            "max_tokens": route.max_tokens,
            "temperature": route.temperature,
            "stream": true
        });
        omit_unset(&mut body, route);
        apply_client_identity(
            client
                .post(endpoint)
                .bearer_auth(credential.token())
                .header("Accept", "text/event-stream")
                .json(&body),
            &route.base_url,
            &route.session_id,
        )
        .build()
        .map_err(|_| CommandError::provider("PROVIDER_REQUEST_INVALID", "无法构造模型请求"))
    }

    /// 流事件统一转换为供应商无关的增量块。
    fn translate(&self, event: &Value) -> Vec<StreamChunk> {
        chat_stream_chunks(event)
    }

    /// 非流响应复用同一增量消费契约。
    fn translate_json(&self, body: &Value) -> Vec<StreamChunk> {
        chat_json_chunks(body)
    }
}

/// 省略未配置的可选字段，避免向供应商发送 null。
fn omit_unset(body: &mut Value, route: &ModelRoute) {
    let Some(object) = body.as_object_mut() else {
        return;
    };
    if route.temperature.is_none() {
        object.remove("temperature");
    }
    if route.max_tokens.is_none() {
        object.remove("max_tokens");
    }
}

/// 工具结果只发文本；同轮调用全部回复后再发带工具身份提示的 user 图片。
fn chat_messages(system: &str, messages: &[Message]) -> Vec<Value> {
    let mut out = vec![json!({"role": "system", "content": system})];
    let mut pending_calls: Vec<&str> = Vec::new();
    let mut tool_images = Vec::new();
    for message in messages {
        match message.role {
            Role::System => out.push(json!({"role": "system", "content": message.text()})),
            Role::User => {
                let has_result = message
                    .blocks
                    .iter()
                    .any(|block| matches!(block, ContentBlock::ToolResult { .. }));
                if has_result {
                    for block in &message.blocks {
                        if let ContentBlock::ToolResult {
                            call_id, blocks, ..
                        } = block
                        {
                            out.push(json!({
                                "role": "tool",
                                "tool_call_id": call_id,
                                "content": blocks_text(blocks)
                            }));
                            pending_calls.retain(|id| *id != call_id.as_str());
                            if let Value::Array(mut content) = blocks_content(blocks) {
                                content[0] = json!({
                                    "type": "text",
                                    "text": format!("以下图片来自工具调用 tool_call_id={call_id} 的结果。")
                                });
                                tool_images.push(json!({"role": "user", "content": content}));
                            }
                        }
                    }
                    // 工具结果可能跨多条中立消息，不能仅按当前消息结束来刷新图片。
                    if pending_calls.is_empty() {
                        out.append(&mut tool_images);
                    }
                } else {
                    out.push(json!({"role": "user", "content": blocks_content(&message.blocks)}));
                }
            }
            Role::Assistant => {
                let calls: Vec<Value> = message
                    .blocks
                    .iter()
                    .filter_map(|block| match block {
                        ContentBlock::ToolCall {
                            id,
                            name,
                            arguments,
                        } => {
                            pending_calls.push(id.as_str());
                            Some(json!({
                                "id": id,
                                "type": "function",
                                "function": {"name": name, "arguments": arguments}
                            }))
                        }
                        _ => None,
                    })
                    .collect();
                let text = message.text();
                let mut value = json!({"role": "assistant"});
                value["content"] = if text.is_empty() {
                    Value::Null
                } else {
                    Value::String(text)
                };
                if !calls.is_empty() {
                    value["tool_calls"] = Value::Array(calls);
                }
                out.push(value);
            }
        }
    }
    out
}

/// 用户内容无图片时用纯字符串，含图片时用内容块数组；不得直接用作 tool 内容。
fn blocks_content(blocks: &[ContentBlock]) -> Value {
    let images: Vec<Value> = blocks
        .iter()
        .filter_map(|block| match block {
            ContentBlock::Image {
                mime, data_base64, ..
            } => Some(json!({
                "type": "image_url",
                "image_url": {"url": format!("data:{mime};base64,{data_base64}")}
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

/// 单个工具定义的 Chat 线格式；strict 约束由 schema 决定。
fn openai_tool_json(tool: &ToolSchema) -> Value {
    json!({
        "type": "function",
        "function": {
            "name": tool.name,
            "description": tool.description,
            "strict": true,
            "parameters": tool.parameters
        }
    })
}

/// SSE 事件到 chunk；一个事件最多产生用量、思考、文本、工具与终止。
fn chat_stream_chunks(event: &Value) -> Vec<StreamChunk> {
    let mut chunks = Vec::new();
    if let Some(error) = event.get("error") {
        chunks.push(error_chunk(error));
        return chunks;
    }
    if let Some(usage) = event.get("usage").and_then(parse_usage) {
        chunks.push(StreamChunk::Usage(usage));
    }
    let Some(choices) = event.get("choices").and_then(Value::as_array) else {
        return chunks;
    };
    for choice in choices {
        let delta = &choice["delta"];
        if let Some(reasoning) = delta
            .get("reasoning_content")
            .or_else(|| delta.get("reasoning"))
            .and_then(Value::as_str)
        {
            if !reasoning.is_empty() {
                chunks.push(StreamChunk::ReasoningDelta {
                    index: REASONING_INDEX,
                    text: reasoning.into(),
                });
            }
        }
        if let Some(text) = delta.get("content").and_then(Value::as_str) {
            if !text.is_empty() {
                chunks.push(StreamChunk::TextDelta {
                    index: TEXT_INDEX,
                    text: text.into(),
                });
            }
        }
        if let Some(calls) = delta.get("tool_calls").and_then(Value::as_array) {
            for call in calls {
                let index = call.get("index").and_then(Value::as_u64).unwrap_or(0) as usize;
                chunks.push(StreamChunk::ToolCallDelta {
                    index: TOOL_INDEX_BASE + index,
                    id: call
                        .get("id")
                        .and_then(Value::as_str)
                        .unwrap_or_default()
                        .into(),
                    item_id: None,
                    name: call
                        .pointer("/function/name")
                        .and_then(Value::as_str)
                        .map(str::to_string),
                    arguments_delta: call
                        .pointer("/function/arguments")
                        .and_then(Value::as_str)
                        .unwrap_or_default()
                        .into(),
                });
            }
        }
        if let Some(reason) = choice.get("finish_reason").and_then(Value::as_str) {
            chunks.push(StreamChunk::Finish {
                reason: chat_finish_reason(reason),
                failure: None,
                replay: None,
            });
        }
    }
    chunks
}

/// 非流式 Chat 响应整体翻译；无工具即正常结束。
fn chat_json_chunks(body: &Value) -> Vec<StreamChunk> {
    let mut chunks = Vec::new();
    if let Some(error) = body.get("error") {
        chunks.push(error_chunk(error));
        return chunks;
    }
    if let Some(usage) = body.get("usage").and_then(parse_usage) {
        chunks.push(StreamChunk::Usage(usage));
    }
    let Some(choices) = body.get("choices").and_then(Value::as_array) else {
        chunks.push(protocol_error_chunk());
        return chunks;
    };
    let mut has_calls = false;
    for choice in choices {
        let message = &choice["message"];
        if let Some(text) = message.get("content").and_then(Value::as_str) {
            if !text.is_empty() {
                chunks.push(StreamChunk::TextDelta {
                    index: TEXT_INDEX,
                    text: text.into(),
                });
            }
        }
        if let Some(calls) = message.get("tool_calls").and_then(Value::as_array) {
            for (index, call) in calls.iter().enumerate() {
                has_calls = true;
                chunks.push(StreamChunk::ToolCallDelta {
                    index: TOOL_INDEX_BASE + index,
                    id: call
                        .get("id")
                        .and_then(Value::as_str)
                        .unwrap_or_default()
                        .into(),
                    item_id: None,
                    name: call
                        .pointer("/function/name")
                        .and_then(Value::as_str)
                        .map(str::to_string),
                    arguments_delta: call
                        .pointer("/function/arguments")
                        .and_then(Value::as_str)
                        .unwrap_or_default()
                        .into(),
                });
            }
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

/// 供应商 in-band 错误；过载与一般传输失败分开归类。
fn error_chunk(error: &Value) -> StreamChunk {
    let code = error.get("code").and_then(Value::as_str);
    let kind = error.get("type").and_then(Value::as_str);
    let overloaded =
        code == Some("server_is_overloaded") || kind == Some("service_unavailable_error");
    let (failure_code, message) = if overloaded {
        (FailureCode::Overloaded, "模型服务当前负载过高，请稍后重试")
    } else {
        (FailureCode::Transport, "供应商流返回失败事件，请稍后重试")
    };
    StreamChunk::Finish {
        reason: FinishReason::Error,
        failure: Some(LlmFailure::provider(failure_code, message)),
        replay: None,
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
fn chat_finish_reason(reason: &str) -> FinishReason {
    match reason {
        "length" => FinishReason::MaxTokens,
        "tool_calls" | "function_call" => FinishReason::ToolCalls,
        _ => FinishReason::Stop,
    }
}

/// 供应商用量解析；缺少 prompt_tokens 视为未提供。
fn parse_usage(value: &Value) -> Option<TokenUsage> {
    let input = value.get("prompt_tokens").and_then(Value::as_u64)?;
    Some(TokenUsage {
        input_tokens: input,
        output_tokens: value
            .get("completion_tokens")
            .and_then(Value::as_u64)
            .unwrap_or(0),
        total_tokens: value.get("total_tokens").and_then(Value::as_u64),
        cache_read_tokens: value
            .pointer("/prompt_tokens_details/cached_tokens")
            .and_then(Value::as_u64),
        cache_write_tokens: None,
        reasoning_tokens: value
            .pointer("/completion_tokens_details/reasoning_tokens")
            .and_then(Value::as_u64),
    })
}

#[cfg(test)]
#[path = "chat_tests.rs"]
mod tests;
