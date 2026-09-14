use super::*;
use crate::ai::llm::assembler::BlockAssembler;
use zeroize::Zeroizing;

/// 测试路由固定 Anthropic Messages。
fn route() -> ModelRoute {
    ModelRoute {
        provider_id: "p".into(),
        protocol: ProviderProtocol::AnthropicMessages,
        auth_type: "api_key",
        model: "claude".into(),
        base_url: "https://example.com/v1".into(),
        max_tokens: Some(512),
        temperature: None,
        parallel_tool_calls: false,
        idle_timeout_ms: 5_000,
        session_id: "session-1".into(),
    }
}

#[test]
/// 工具结果的图片放进 tool_result 内容块数组，Anthropic 才允许回图。
fn tool_results_carry_images_as_content_blocks() {
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
    let wire = anthropic_messages(std::slice::from_ref(&message));
    let content = &wire[0]["content"][0];
    assert_eq!(content["type"], "tool_result");
    assert_eq!(content["content"][0]["text"], "已截帧");
    assert_eq!(content["content"][1]["type"], "image");
    assert_eq!(content["content"][1]["source"]["media_type"], "image/jpeg");
}

#[test]
/// 请求不重复拼接 v1，工具使用 input_schema，鉴权走 x-api-key。
fn builds_anthropic_request_contract() {
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
    let wire = Anthropic
        .build(
            &client,
            &route(),
            &request,
            &Credential::ApiKey(Zeroizing::new("secret".into())),
        )
        .unwrap();
    assert_eq!(wire.url().as_str(), "https://example.com/v1/messages");
    assert_eq!(wire.headers().get("x-api-key").unwrap(), "secret");
    let body: Value = serde_json::from_slice(wire.body().unwrap().as_bytes().unwrap()).unwrap();
    assert_eq!(body["tools"][0]["input_schema"]["type"], "object");
    assert_eq!(body["max_tokens"], 512);
    assert_eq!(body["stream"], true);
    assert_eq!(body["tool_choice"]["name"], "emit");
    assert_eq!(body["tool_choice"]["disable_parallel_tool_use"], true);
    assert_eq!(body["messages"][0]["content"][0]["text"], "你好");
}

#[test]
/// 工具调用跨事件拼接参数，message_delta 给出 tool_use 结束原因。
fn folds_tool_use_deltas() {
    let mut assembler = BlockAssembler::default();
    for event in [
        json!({"type": "content_block_start", "index": 1, "content_block": {"type": "tool_use", "id": "toolu_1", "name": "emit"}}),
        json!({"type": "content_block_delta", "index": 1, "delta": {"type": "input_json_delta", "partial_json": "{\"schema_version\":1}"}}),
        json!({"type": "message_delta", "delta": {"stop_reason": "tool_use"}, "usage": {"output_tokens": 9}}),
    ] {
        for chunk in Anthropic.translate(&event) {
            assembler.push(chunk);
        }
    }
    assert_eq!(assembler.finish().unwrap().0, FinishReason::ToolCalls);
    let calls = assembler.tool_calls();
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].id, "toolu_1");
    assert_eq!(calls[0].arguments, "{\"schema_version\":1}");
    assert_eq!(assembler.usage().unwrap().output_tokens, 9);
}

#[test]
/// 思考与文本分块累积，message_stop 被识别为终止事件。
fn separates_thinking_and_text() {
    let mut assembler = BlockAssembler::default();
    for event in [
        json!({"type": "content_block_delta", "index": 0, "delta": {"type": "thinking_delta", "thinking": "想过"}}),
        json!({"type": "content_block_delta", "index": 1, "delta": {"type": "text_delta", "text": "答案"}}),
    ] {
        for chunk in Anthropic.translate(&event) {
            assembler.push(chunk);
        }
    }
    let blocks = assembler.blocks();
    assert_eq!(blocks.len(), 2);
    assert!(matches!(blocks[0], ContentBlock::Reasoning { .. }));
    assert!(matches!(blocks[1], ContentBlock::Text { .. }));
    assert!(Anthropic.is_terminal(&json!({"type": "message_stop"})));
    assert!(!Anthropic.is_terminal(&json!({"type": "message_delta"})));
}

#[test]
/// 非流式响应与流式折叠出同一结果类型。
fn translates_anthropic_json_body() {
    let body = json!({
        "content": [
            {"type": "text", "text": "看词典"},
            {"type": "tool_use", "id": "toolu_2", "name": "lookup_words", "input": {"words": ["speak"]}}
        ],
        "usage": {"input_tokens": 5, "output_tokens": 3}
    });
    let mut assembler = BlockAssembler::default();
    for chunk in Anthropic.translate_json(&body) {
        assembler.push(chunk);
    }
    assert_eq!(assembler.finish().unwrap().0, FinishReason::ToolCalls);
    assert_eq!(assembler.tool_calls()[0].name, "lookup_words");
    assert_eq!(assembler.usage().unwrap().input_tokens, 5);
}

#[test]
/// overloaded_error 归类为可重试错误。
fn classifies_overloaded_error() {
    let chunks = Anthropic.translate(&json!({
        "type": "error",
        "error": {"type": "overloaded_error", "message": "overloaded"}
    }));
    match &chunks[0] {
        StreamChunk::Finish {
            failure: Some(failure),
            ..
        } => {
            assert_eq!(failure.code, FailureCode::Overloaded);
            assert!(failure.code.retryable());
        }
        other => panic!("预期失败终止，得到 {other:?}"),
    }
}

#[test]
/// 未配置输出上限时协议请求沿用统一的 32K 默认值。
fn defaults_output_limit_to_32k() {
    let mut route = route();
    route.max_tokens = None;
    let request = ModelRequest {
        system: "sys".into(),
        messages: vec![Message::user_text("m1", "你好")],
        tools: vec![],
        tool_choice: None,
    };
    let wire = Anthropic
        .build(
            &Client::new(),
            &route,
            &request,
            &Credential::ApiKey(Zeroizing::new("secret".into())),
        )
        .unwrap();
    let body: Value = serde_json::from_slice(wire.body().unwrap().as_bytes().unwrap()).unwrap();
    assert_eq!(body["max_tokens"], 32_768);
}
