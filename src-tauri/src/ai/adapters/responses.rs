//! OpenAI Responses 适配器：ChatGPT OAuth 无状态续传与 SSE/JSON 到 chunk 的翻译。

use reqwest::{Client, Request};
use serde_json::{json, Value};

use crate::ai::llm::adapter::LlmAdapter;
use crate::ai::llm::chunk::{ReplayEnvelope, StreamChunk};
use crate::ai::llm::failure::{FailureCode, LlmFailure};
use crate::ai::llm::request::{Credential, ModelRequest, ModelRoute, ToolSchema};
use crate::ai::llm::vocabulary::{ContentBlock, FinishReason, Message, Role, TokenUsage};
use crate::ai::{apply_client_identity, USER_AGENT};
use crate::error::CommandError;
use crate::models::OPENAI_SUBSCRIPTION_ENDPOINT;

pub(crate) struct Responses;

impl LlmAdapter for Responses {
    fn protocol(&self) -> &'static str {
        "openai_responses"
    }

    fn build(
        &self,
        client: &Client,
        route: &ModelRoute,
        request: &ModelRequest,
        credential: &Credential,
    ) -> Result<Request, CommandError> {
        let mut body = json!({
            "model": route.model,
            "instructions": request.system,
            "input": responses_input(&request.messages),
            "tools": request.tools.iter().map(responses_tool_json).collect::<Vec<_>>(),
            "parallel_tool_calls": route.parallel_tool_calls,
            "store": false,
            "stream": true
        });
        if let Some(name) = &request.tool_choice {
            body["tool_choice"] = json!({"type": "function", "name": name});
        }
        let mut builder = client
            .post(OPENAI_SUBSCRIPTION_ENDPOINT)
            .bearer_auth(credential.token())
            .header("originator", "quailcard")
            .header("User-Agent", USER_AGENT)
            .header("Accept", "text/event-stream")
            .json(&body);
        if let Some(account) = credential.account_id() {
            builder = builder.header("ChatGPT-Account-Id", account);
        }
        apply_client_identity(builder, &route.base_url, &route.session_id)
            .build()
            .map_err(|_| CommandError::provider("PROVIDER_REQUEST_INVALID", "无法构造模型请求"))
    }

    fn translate(&self, event: &Value) -> Vec<StreamChunk> {
        responses_chunks(event)
    }

    fn translate_json(&self, body: &Value) -> Vec<StreamChunk> {
        responses_json_chunks(body)
    }

    /// Responses 的收束事件：completed / incomplete / failed / error。
    fn is_terminal(&self, event: &Value) -> bool {
        matches!(
            event.get("type").and_then(Value::as_str),
            Some("response.completed")
                | Some("response.incomplete")
                | Some("response.failed")
                | Some("error")
        )
    }
}

/// 组装 Responses input：adapter 私有重放项优先，工具结果单独成项。
fn responses_input(messages: &[Message]) -> Vec<Value> {
    let mut input = Vec::new();
    for message in messages {
        if message.role == Role::System {
            continue;
        }
        if let Some(items) = message.replay.as_ref().and_then(Value::as_array) {
            input.extend(items.iter().cloned());
            continue;
        }
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
                    input.push(json!({
                        "type": "function_call_output",
                        "call_id": call_id,
                        "output": blocks_text(blocks)
                    }));
                    // Responses 的 function_call_output 只接受字符串，
                    // 工具截图必须跟在后面的 user 消息里用 input_image 送回。
                    let images = image_items(blocks);
                    if !images.is_empty() {
                        let mut content =
                            vec![json!({"type": "input_text", "text": "（工具返回的画面）"})];
                        content.extend(images);
                        input.push(json!({"role": "user", "content": content}));
                    }
                }
            }
            continue;
        }
        let role = match message.role {
            Role::Assistant => "assistant",
            _ => "user",
        };
        input.push(json!({"role": role, "content": responses_content(&message.blocks)}));
    }
    input
}

/// 用户内容使用 input_text/input_image；assistant 文本使用 output_text。
fn responses_content(blocks: &[ContentBlock]) -> Vec<Value> {
    let mut content = Vec::new();
    for block in blocks {
        if let ContentBlock::Text { text } = block {
            if !text.is_empty() {
                content.push(json!({"type": "input_text", "text": text}));
            }
        }
    }
    content.extend(image_items(blocks));
    if content.is_empty() {
        content.push(json!({"type": "input_text", "text": "（无文字）"}));
    }
    content
}

/// input_image 块；用户消息与工具回图共用同一形状。
fn image_items(blocks: &[ContentBlock]) -> Vec<Value> {
    blocks
        .iter()
        .filter_map(|block| match block {
            ContentBlock::Image {
                mime, data_base64, ..
            } => Some(json!({
                "type": "input_image",
                "image_url": format!("data:{mime};base64,{data_base64}")
            })),
            _ => None,
        })
        .collect()
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

/// Responses 工具定义使用顶层 name/parameters。
fn responses_tool_json(tool: &ToolSchema) -> Value {
    json!({
        "type": "function",
        "name": tool.name,
        "description": tool.description,
        "parameters": tool.parameters,
        "strict": true
    })
}

/// 事件序号；Responses 事件用 output_index 关联输出项。
fn output_index(event: &Value) -> usize {
    event
        .get("output_index")
        .and_then(Value::as_u64)
        .unwrap_or(0) as usize
}

/// SSE 事件到 chunk。
fn responses_chunks(event: &Value) -> Vec<StreamChunk> {
    let mut chunks = Vec::new();
    let kind = event
        .get("type")
        .and_then(Value::as_str)
        .unwrap_or_default();
    match kind {
        "response.output_item.added" => {
            if event.pointer("/item/type").and_then(Value::as_str) == Some("function_call") {
                chunks.push(StreamChunk::ToolCallDelta {
                    index: output_index(event),
                    id: event
                        .pointer("/item/call_id")
                        .and_then(Value::as_str)
                        .unwrap_or_default()
                        .into(),
                    item_id: event
                        .pointer("/item/id")
                        .and_then(Value::as_str)
                        .map(str::to_string),
                    name: event
                        .pointer("/item/name")
                        .and_then(Value::as_str)
                        .map(str::to_string),
                    arguments_delta: String::new(),
                });
            }
        }
        "response.function_call_arguments.delta" => {
            if let Some(delta) = event.get("delta").and_then(Value::as_str) {
                chunks.push(StreamChunk::ToolCallDelta {
                    index: output_index(event),
                    id: String::new(),
                    item_id: event
                        .get("item_id")
                        .and_then(Value::as_str)
                        .map(str::to_string),
                    name: None,
                    arguments_delta: delta.into(),
                });
            }
        }
        "response.output_text.delta" => {
            if let Some(delta) = event.get("delta").and_then(Value::as_str) {
                if !delta.is_empty() {
                    chunks.push(StreamChunk::TextDelta {
                        index: output_index(event),
                        text: delta.into(),
                    });
                }
            }
        }
        "response.reasoning_summary_text.delta" | "response.reasoning_text.delta" => {
            if let Some(delta) = event.get("delta").and_then(Value::as_str) {
                if !delta.is_empty() {
                    chunks.push(StreamChunk::ReasoningDelta {
                        index: output_index(event),
                        text: delta.into(),
                    });
                }
            }
        }
        "response.completed" => {
            if let Some(usage) = event
                .pointer("/response/usage")
                .and_then(parse_responses_usage)
            {
                chunks.push(StreamChunk::Usage(usage));
            }
            let output = event.pointer("/response/output").and_then(Value::as_array);
            let has_calls = output.is_some_and(|items| {
                items
                    .iter()
                    .any(|item| item.get("type").and_then(Value::as_str) == Some("function_call"))
            });
            let replay = output.map(|items| ReplayEnvelope {
                response: Some(Value::Array(items.clone())),
                blocks: Vec::new(),
            });
            chunks.push(StreamChunk::Finish {
                reason: if has_calls {
                    FinishReason::ToolCalls
                } else {
                    FinishReason::Stop
                },
                failure: None,
                replay,
            });
        }
        "response.incomplete" => {
            chunks.push(StreamChunk::Finish {
                reason: FinishReason::Error,
                failure: Some(LlmFailure::provider(
                    FailureCode::ResponseIncomplete,
                    "供应商流式响应在正常结束前中断",
                )),
                replay: None,
            });
        }
        "response.failed" | "error" => {
            chunks.push(StreamChunk::Finish {
                reason: FinishReason::Error,
                failure: Some(responses_failure(event)),
                replay: None,
            });
        }
        _ => {}
    }
    chunks
}

/// 非流式响应整体翻译；无工具即正常结束。
fn responses_json_chunks(body: &Value) -> Vec<StreamChunk> {
    let mut chunks = Vec::new();
    if let Some(usage) = body.get("usage").and_then(parse_responses_usage) {
        chunks.push(StreamChunk::Usage(usage));
    }
    let Some(items) = body.get("output").and_then(Value::as_array) else {
        chunks.push(protocol_error_chunk());
        return chunks;
    };
    let mut has_calls = false;
    for (index, item) in items.iter().enumerate() {
        match item.get("type").and_then(Value::as_str) {
            Some("function_call") => {
                has_calls = true;
                chunks.push(StreamChunk::ToolCallDelta {
                    index,
                    id: item
                        .get("call_id")
                        .and_then(Value::as_str)
                        .unwrap_or_default()
                        .into(),
                    item_id: item.get("id").and_then(Value::as_str).map(str::to_string),
                    name: item.get("name").and_then(Value::as_str).map(str::to_string),
                    arguments_delta: item
                        .get("arguments")
                        .and_then(Value::as_str)
                        .unwrap_or_default()
                        .into(),
                });
            }
            Some("message") => {
                if let Some(content) = item.get("content").and_then(Value::as_array) {
                    for block in content {
                        if let Some(text) = block.get("text").and_then(Value::as_str) {
                            if !text.is_empty() {
                                chunks.push(StreamChunk::TextDelta {
                                    index,
                                    text: text.into(),
                                });
                            }
                        }
                    }
                }
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

/// in-band 错误归类；过载可重试。
fn responses_failure(event: &Value) -> LlmFailure {
    let code = event
        .pointer("/response/error/code")
        .or_else(|| event.pointer("/error/code"))
        .and_then(Value::as_str);
    let kind = event
        .pointer("/response/error/type")
        .or_else(|| event.pointer("/error/type"))
        .and_then(Value::as_str);
    if code == Some("server_is_overloaded") || kind == Some("service_unavailable_error") {
        LlmFailure::provider(
            FailureCode::Overloaded,
            "ChatGPT Codex 当前负载过高，请稍后重试",
        )
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

/// Responses 用量字段为 input_tokens/output_tokens。
fn parse_responses_usage(value: &Value) -> Option<TokenUsage> {
    let input = value.get("input_tokens").and_then(Value::as_u64)?;
    Some(TokenUsage {
        input_tokens: input,
        output_tokens: value
            .get("output_tokens")
            .and_then(Value::as_u64)
            .unwrap_or(0),
        total_tokens: value.get("total_tokens").and_then(Value::as_u64),
        cache_read_tokens: value
            .pointer("/input_tokens_details/cached_tokens")
            .and_then(Value::as_u64),
        cache_write_tokens: None,
        reasoning_tokens: value
            .pointer("/output_tokens_details/reasoning_tokens")
            .and_then(Value::as_u64),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ai::llm::assembler::BlockAssembler;
    use crate::ai::ProviderProtocol;
    use zeroize::Zeroizing;

    /// 测试路由固定 ChatGPT OAuth + Responses。
    fn route() -> ModelRoute {
        ModelRoute {
            provider_id: "p".into(),
            protocol: ProviderProtocol::OpenAiCompatible,
            auth_type: "openai_oauth",
            model: "gpt-5".into(),
            base_url: "https://example.com/v1".into(),
            max_tokens: None,
            temperature: None,
            parallel_tool_calls: true,
            idle_timeout_ms: 5_000,
            session_id: "session-1".into(),
        }
    }

    #[test]
    /// 请求固定 Codex 端点、账号路由头、store=false 与顶层工具 schema。
    fn builds_responses_request_contract() {
        let client = reqwest::Client::new();
        let request = ModelRequest {
            system: "sys".into(),
            messages: vec![Message::user_text("m1", "你好")],
            tools: vec![ToolSchema {
                name: "emit".into(),
                description: "d".into(),
                parameters: json!({"type": "object"}),
            }],
            tool_choice: Some("emit".into()),
        };
        let wire = Responses
            .build(
                &client,
                &route(),
                &request,
                &Credential::OAuth {
                    access_token: Zeroizing::new("token".into()),
                    account_id: Some("acct".into()),
                },
            )
            .unwrap();
        assert_eq!(wire.url().as_str(), OPENAI_SUBSCRIPTION_ENDPOINT);
        assert_eq!(wire.headers().get("ChatGPT-Account-Id").unwrap(), "acct");
        let body: Value = serde_json::from_slice(wire.body().unwrap().as_bytes().unwrap()).unwrap();
        assert_eq!(body["store"], false);
        assert_eq!(body["tools"][0]["name"], "emit");
        assert_eq!(body["tool_choice"]["type"], "function");
        assert_eq!(body["input"][0]["content"][0]["type"], "input_text");
    }

    #[test]
    /// function_call_output 只收文本，工具截图另起一条 user 消息用 input_image 送回。
    fn tool_results_carry_images_as_follow_up_message() {
        let message = Message {
            id: "m1".into(),
            role: Role::User,
            blocks: vec![ContentBlock::ToolResult {
                call_id: "c1".into(),
                blocks: vec![
                    ContentBlock::Text {
                        text: "已截帧".into(),
                    },
                    ContentBlock::Image {
                        name: String::new(),
                        mime: "image/jpeg".into(),
                        data_base64: "AAA".into(),
                    },
                ],
                is_error: false,
            }],
            source: None,
            replay: None,
        };
        let wire = responses_input(std::slice::from_ref(&message));
        assert_eq!(wire[0]["type"], "function_call_output");
        assert_eq!(wire[0]["output"], "已截帧");
        assert_eq!(wire[1]["role"], "user");
        assert_eq!(wire[1]["content"][1]["type"], "input_image");
        assert_eq!(
            wire[1]["content"][1]["image_url"],
            "data:image/jpeg;base64,AAA"
        );
    }

    #[test]
    /// function_call 增量与 completed 折叠出带 call_id/item_id 的调用。
    fn folds_function_call_stream() {
        let mut assembler = BlockAssembler::default();
        for event in [
            json!({"type": "response.output_item.added", "output_index": 0, "item": {"type": "function_call", "id": "fc_1", "call_id": "call_1", "name": "emit"}}),
            json!({"type": "response.function_call_arguments.delta", "output_index": 0, "item_id": "fc_1", "delta": "{\"schema_version\":1}"}),
            json!({"type": "response.completed", "response": {"output": [{"type": "function_call", "id": "fc_1", "call_id": "call_1", "name": "emit", "arguments": "{\"schema_version\":1}"}], "usage": {"input_tokens": 4, "output_tokens": 2}}}),
        ] {
            for chunk in Responses.translate(&event) {
                assembler.push(chunk);
            }
        }
        assert_eq!(assembler.finish().unwrap().0, FinishReason::ToolCalls);
        let calls = assembler.tool_calls();
        assert_eq!(calls[0].id, "call_1");
        assert_eq!(calls[0].item_id.as_deref(), Some("fc_1"));
        assert_eq!(calls[0].arguments, "{\"schema_version\":1}");
        assert_eq!(assembler.usage().unwrap().input_tokens, 4);
        assert!(assembler.replay().is_some());
    }

    #[test]
    /// incomplete 归类为响应不完整错误，且被识别为终止事件。
    fn classifies_incomplete_stream() {
        let chunks = Responses.translate(&json!({"type": "response.incomplete"}));
        match &chunks[0] {
            StreamChunk::Finish {
                failure: Some(failure),
                ..
            } => assert_eq!(failure.code, FailureCode::ResponseIncomplete),
            other => panic!("预期失败终止，得到 {other:?}"),
        }
        assert!(Responses.is_terminal(&json!({"type": "response.completed"})));
        assert!(!Responses.is_terminal(&json!({"type": "response.output_text.delta"})));
    }

    #[test]
    /// 非流式响应与流式折叠出同一结果类型。
    fn translates_responses_json_body() {
        let body = json!({
            "output": [
                {"type": "message", "content": [{"type": "output_text", "text": "看词典"}]},
                {"type": "function_call", "id": "fc_2", "call_id": "call_2", "name": "lookup_words", "arguments": "{\"words\":[\"speak\"]}"}
            ],
            "usage": {"input_tokens": 6, "output_tokens": 3}
        });
        let mut assembler = BlockAssembler::default();
        for chunk in Responses.translate_json(&body) {
            assembler.push(chunk);
        }
        assert_eq!(assembler.finish().unwrap().0, FinishReason::ToolCalls);
        assert_eq!(assembler.tool_calls()[0].item_id.as_deref(), Some("fc_2"));
        assert_eq!(assembler.usage().unwrap().output_tokens, 3);
    }
}
