use super::*;
use crate::ai::llm::assembler::BlockAssembler;
use zeroize::Zeroizing;

/// 测试路由固定走 OpenAI Compatible。
fn route(base_url: &str) -> ModelRoute {
    ModelRoute {
        provider_id: "p".into(),
        protocol: ProviderProtocol::OpenAiCompatible,
        auth_type: "api_key",
        model: "test".into(),
        base_url: base_url.into(),
        max_tokens: Some(100),
        temperature: Some(0.2),
        parallel_tool_calls: true,
        idle_timeout_ms: 5_000,
        session_id: "session-1".into(),
    }
}

#[test]
/// 请求使用版本端点、Bearer 认证、strict 工具与并行开关。
fn builds_chat_request_contract() {
    let client = reqwest::Client::new();
    let request = ModelRequest {
        system: "sys".into(),
        messages: vec![Message::user_text("m1", "你好")],
        tools: vec![ToolSchema {
            name: "emit".into(),
            description: "d".into(),
            parameters: json!({"type": "object"}),
        }],
        tool_choice: None,
    };
    let wire = Chat
        .build(
            &client,
            &route("https://example.com/v1"),
            &request,
            &Credential::ApiKey(Zeroizing::new("secret".into())),
        )
        .unwrap();
    assert_eq!(
        wire.url().as_str(),
        "https://example.com/v1/chat/completions"
    );
    assert_eq!(
        wire.headers().get("Authorization").unwrap(),
        "Bearer secret"
    );
    let body: Value = serde_json::from_slice(wire.body().unwrap().as_bytes().unwrap()).unwrap();
    assert_eq!(body["tools"][0]["function"]["strict"], true);
    assert_eq!(body["parallel_tool_calls"], true);
    assert_eq!(body["stream"], true);
    assert_eq!(body["messages"][1]["content"], "你好");
}

/// 构造消息夹具，避免来源与重放字段干扰线格式断言。
fn message(role: Role, blocks: Vec<ContentBlock>) -> Message {
    Message {
        id: "m".into(),
        role,
        blocks,
        source: None,
        replay: None,
    }
}

/// 同轮调用使用不同身份，即使函数名相同也能区分图片归属。
fn assistant_calls(ids: &[&str]) -> Message {
    message(
        Role::Assistant,
        ids.iter()
            .map(|id| ContentBlock::ToolCall {
                id: (*id).into(),
                name: "capture_frame".into(),
                arguments: "{}".into(),
            })
            .collect(),
    )
}

/// 工具结果夹具支持纯文本、纯图片与多图片，保留原始 MIME 和载荷。
fn tool_result(id: &str, text: &str, images: &[(&str, &str)]) -> ContentBlock {
    let mut blocks = vec![ContentBlock::Text { text: text.into() }];
    blocks.extend(images.iter().map(|(mime, data)| ContentBlock::Image {
        name: "frame".into(),
        mime: (*mime).into(),
        data_base64: (*data).into(),
    }));
    ContentBlock::ToolResult {
        call_id: id.into(),
        blocks,
        is_error: false,
    }
}

#[test]
/// 单工具结果保持纯文本，多张图以随后 user 消息发送并标注调用身份。
fn tool_results_send_images_in_following_user_message() {
    let wire = chat_messages(
        "sys",
        &[
            assistant_calls(&["c1"]),
            message(
                Role::User,
                vec![tool_result(
                    "c1",
                    "已截帧",
                    &[("image/jpeg", "AAA"), ("image/png", "BBB")],
                )],
            ),
        ],
    );
    assert_eq!(wire.len(), 4);
    assert_eq!(
        wire[2],
        json!({"role": "tool", "tool_call_id": "c1", "content": "已截帧"})
    );
    assert_eq!(wire[3]["role"], "user");
    assert_eq!(wire[3]["content"][0]["type"], "text");
    assert!(wire[3]["content"][0]["text"]
        .as_str()
        .unwrap()
        .contains("tool_call_id=c1"));
    assert_eq!(
        wire[3]["content"][1],
        json!({"type": "image_url", "image_url": {"url": "data:image/jpeg;base64,AAA"}})
    );
    assert_eq!(
        wire[3]["content"][2],
        json!({"type": "image_url", "image_url": {"url": "data:image/png;base64,BBB"}})
    );
}

#[test]
/// 并行结果不论集中还是分条返回，都先完整回复再发送图片，且按 ID 关联而非返回位置。
fn parallel_tool_images_wait_for_all_responses() {
    for separate_messages in [false, true] {
        let results = vec![
            tool_result("c2", "第二张", &[("image/png", "BBB")]),
            tool_result("c1", "第一张", &[("image/jpeg", "AAA")]),
            tool_result("c3", "纯文本", &[]),
        ];
        let mut messages = vec![assistant_calls(&["c1", "c2", "c3"])];
        if separate_messages {
            messages.extend(
                results
                    .into_iter()
                    .map(|result| message(Role::User, vec![result])),
            );
        } else {
            messages.push(message(Role::User, results));
        }
        messages.push(Message::user_text("next", "继续"));
        let wire = chat_messages("sys", &messages);
        let roles: Vec<_> = wire
            .iter()
            .map(|value| value["role"].as_str().unwrap())
            .collect();
        assert_eq!(
            roles,
            [
                "system",
                "assistant",
                "tool",
                "tool",
                "tool",
                "user",
                "user",
                "user"
            ]
        );
        for (offset, (id, text)) in [("c2", "第二张"), ("c1", "第一张"), ("c3", "纯文本")]
            .iter()
            .enumerate()
        {
            assert_eq!(
                wire[offset + 2],
                json!({"role": "tool", "tool_call_id": id, "content": text})
            );
        }
        for (offset, (id, url)) in [
            ("c2", "data:image/png;base64,BBB"),
            ("c1", "data:image/jpeg;base64,AAA"),
        ]
        .iter()
        .enumerate()
        {
            assert!(wire[offset + 5]["content"][0]["text"]
                .as_str()
                .unwrap()
                .contains(&format!("tool_call_id={id}")));
            assert_eq!(wire[offset + 5]["content"][1]["image_url"]["url"], *url);
        }
        assert_eq!(wire[7]["content"], "继续");
    }
}

#[test]
/// 纯文本结果不生成额外 user 消息，纯图结果仍提供合法的空文本 tool 响应。
fn text_only_and_image_only_tool_results_remain_valid() {
    for (text, images, expected_len) in [("完成", vec![], 3), ("", vec![("image/jpeg", "AAA")], 4)]
    {
        let wire = chat_messages(
            "sys",
            &[
                assistant_calls(&["c1"]),
                message(Role::User, vec![tool_result("c1", text, &images)]),
            ],
        );
        assert_eq!(wire.len(), expected_len);
        assert_eq!(wire[2]["content"], text);
    }
}

#[test]
/// 未完整返回的并行调用不能因历史到尾而提前刷新图片。
fn incomplete_parallel_results_do_not_flush_images() {
    let wire = chat_messages(
        "sys",
        &[
            assistant_calls(&["c1", "c2"]),
            message(
                Role::User,
                vec![tool_result("c1", "已截帧", &[("image/jpeg", "AAA")])],
            ),
        ],
    );
    assert_eq!(wire.len(), 3);
    assert_eq!(wire[2]["role"], "tool");
    assert_eq!(wire[2]["content"], "已截帧");
}

#[test]
/// OpenCode 端点附带稳定会话头，其他域名不附带。
fn scopes_opencode_session_header() {
    let client = reqwest::Client::new();
    let request = ModelRequest {
        system: "s".into(),
        messages: Vec::new(),
        tools: Vec::new(),
        tool_choice: None,
    };
    let opencode = Chat
        .build(
            &client,
            &route("https://opencode.ai/zen/go/v1"),
            &request,
            &Credential::ApiKey(Zeroizing::new("k".into())),
        )
        .unwrap();
    assert_eq!(
        opencode.headers().get("x-opencode-session").unwrap(),
        "session-1"
    );
    let other = Chat
        .build(
            &client,
            &route("https://example.com/v1"),
            &request,
            &Credential::ApiKey(Zeroizing::new("k".into())),
        )
        .unwrap();
    assert!(other.headers().get("x-opencode-session").is_none());
}

#[test]
/// SSE 事件翻译出思考、文本、工具增量与用量，参数保持原始字符串。
fn translates_chat_sse_event() {
    let event = json!({
        "choices": [{
            "delta": {
                "reasoning_content": "想想",
                "content": "你好",
                "tool_calls": [{
                    "index": 0,
                    "id": "call_1",
                    "function": {"name": "emit", "arguments": "{\"a\":"}
                }]
            },
            "finish_reason": null
        }],
        "usage": {"prompt_tokens": 3, "completion_tokens": 2}
    });
    let chunks = Chat.translate(&event);
    assert!(chunks
        .iter()
        .any(|chunk| matches!(chunk, StreamChunk::Usage(usage) if usage.input_tokens == 3)));
    assert!(chunks
        .iter()
        .any(|chunk| matches!(chunk, StreamChunk::ReasoningDelta { .. })));
    assert!(chunks
        .iter()
        .any(|chunk| matches!(chunk, StreamChunk::TextDelta { text, .. } if text == "你好")));
    assert!(chunks.iter().any(|chunk| matches!(
        chunk,
        StreamChunk::ToolCallDelta { name, arguments_delta, .. }
            if name.as_deref() == Some("emit") && arguments_delta == "{\"a\":"
    )));
}

#[test]
/// 非流式响应与流式响应折叠出同一结果类型。
fn translates_chat_json_body() {
    let body = json!({
        "choices": [{"message": {"content": "", "tool_calls": [{
            "id": "call_1",
            "type": "function",
            "function": {"name": "emit", "arguments": "{\"a\":1}"}
        }]}}]
    });
    let mut assembler = BlockAssembler::default();
    for chunk in Chat.translate_json(&body) {
        assembler.push(chunk);
    }
    assert_eq!(assembler.finish().unwrap().0, FinishReason::ToolCalls);
    let calls = assembler.tool_calls();
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].name, "emit");
    assert_eq!(calls[0].arguments, "{\"a\":1}");
}

#[test]
/// 过载错误归类为可重试的 OVERLOADED，其他 in-band 错误为传输失败。
fn classifies_in_band_errors() {
    let overloaded = Chat.translate(&json!({
        "error": {"code": "server_is_overloaded", "type": "service_unavailable_error"}
    }));
    match &overloaded[0] {
        StreamChunk::Finish {
            failure: Some(failure),
            ..
        } => {
            assert_eq!(failure.code, FailureCode::Overloaded);
            assert!(failure.code.retryable());
        }
        other => panic!("预期失败终止，得到 {other:?}"),
    }
    let other = Chat.translate(&json!({"error": {"type": "api_error"}}));
    match &other[0] {
        StreamChunk::Finish {
            failure: Some(failure),
            ..
        } => {
            assert_eq!(failure.code, FailureCode::Transport);
        }
        other => panic!("预期失败终止，得到 {other:?}"),
    }
}
