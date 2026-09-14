//! 三协议适配器的统一契约测试：同一逻辑对话必须在三条 wire 通道上得到等价结果。

use serde_json::{json, Value};
use zeroize::Zeroizing;

use super::{anthropic::Anthropic, chat::Chat, responses::Responses};
use crate::ai::llm::adapter::LlmAdapter;
use crate::ai::llm::assembler::BlockAssembler;
use crate::ai::llm::failure::FailureCode;
use crate::ai::llm::request::{Credential, ModelRequest, ModelRoute, ToolSchema};
use crate::ai::llm::vocabulary::{FinishReason, Message};
use crate::ai::ProviderProtocol;

/// 测试用工具线格式。
fn tool() -> ToolSchema {
    ToolSchema {
        name: "emit".into(),
        description: "输出".into(),
        parameters: json!({"type": "object", "properties": {"a": {"type": "integer"}}}),
    }
}

/// 测试用中立请求。
fn request() -> ModelRequest {
    ModelRequest {
        system: "sys".into(),
        messages: vec![Message::user_text("m1", "你好")],
        tools: vec![tool()],
        tool_choice: None,
    }
}

/// 一条协议通道的夹具：路由、凭据、SSE 事件与 JSON 体。
struct Case {
    adapter: &'static dyn LlmAdapter,
    route: ModelRoute,
    credential: Credential,
    stream_events: Vec<Value>,
    json_body: Value,
}

/// 构造固定路由。
fn route(protocol: ProviderProtocol, auth_type: &'static str) -> ModelRoute {
    ModelRoute {
        provider_id: "p".into(),
        protocol,
        auth_type,
        model: "test".into(),
        base_url: "https://example.com/v1".into(),
        max_tokens: Some(100),
        temperature: Some(0.2),
        parallel_tool_calls: false,
        idle_timeout_ms: 5_000,
        session_id: "session-1".into(),
    }
}

/// 三条协议的等价夹具；参数片段共用同一份 JSON。
fn cases() -> Vec<Case> {
    let args = json!({"a": 1}).to_string();
    vec![
        Case {
            adapter: &Chat,
            route: route(ProviderProtocol::OpenAiCompatible, "api_key"),
            credential: Credential::ApiKey(Zeroizing::new("secret".into())),
            stream_events: vec![
                json!({"choices":[{"delta":{"tool_calls":[{"index":0,"id":"call_1","function":{"name":"emit"}}]}}]}),
                json!({"choices":[{"delta":{"tool_calls":[{"index":0,"function":{"arguments": &args}}]},"finish_reason":"tool_calls"}]}),
            ],
            json_body: json!({"choices":[{"message":{"content":"","tool_calls":[{"id":"call_1","type":"function","function":{"name":"emit","arguments": &args}}]}}]}),
        },
        Case {
            adapter: &Anthropic,
            route: route(ProviderProtocol::AnthropicMessages, "api_key"),
            credential: Credential::ApiKey(Zeroizing::new("secret".into())),
            stream_events: vec![
                json!({"type":"content_block_start","index":0,"content_block":{"type":"tool_use","id":"call_1","name":"emit"}}),
                json!({"type":"content_block_delta","index":0,"delta":{"type":"input_json_delta","partial_json": &args}}),
                json!({"type":"message_delta","delta":{"stop_reason":"tool_use"}}),
            ],
            json_body: json!({"content":[{"type":"tool_use","id":"call_1","name":"emit","input": {"a": 1}}]}),
        },
        Case {
            adapter: &Responses,
            route: route(ProviderProtocol::OpenAiCompatible, "openai_oauth"),
            credential: Credential::OAuth {
                access_token: Zeroizing::new("token".into()),
                account_id: None,
            },
            stream_events: vec![
                json!({"type":"response.output_item.added","output_index":0,"item":{"type":"function_call","id":"fc_1","call_id":"call_1","name":"emit"}}),
                json!({"type":"response.function_call_arguments.delta","output_index":0,"item_id":"fc_1","delta": &args}),
                json!({"type":"response.completed","response":{"output":[{"type":"function_call","id":"fc_1","call_id":"call_1","name":"emit","arguments": &args}]}}),
            ],
            json_body: json!({"output":[{"type":"function_call","id":"fc_1","call_id":"call_1","name":"emit","arguments": &args}]}),
        },
    ]
}

/// 折叠出的工具调用与结束原因必须跨协议一致。
fn assert_call(adapter: &dyn LlmAdapter, assembler: &BlockAssembler, label: &str) {
    let calls = assembler.tool_calls();
    assert_eq!(calls.len(), 1, "{} {label} 调用数", adapter.protocol());
    assert_eq!(
        calls[0].name,
        "emit",
        "{} {label} 工具名",
        adapter.protocol()
    );
    let value: Value = serde_json::from_str(&calls[0].arguments).expect("参数不是 JSON");
    assert_eq!(value["a"], 1, "{} {label} 参数", adapter.protocol());
    assert_eq!(
        assembler.finish().map(|(reason, _)| reason),
        Some(FinishReason::ToolCalls),
        "{} {label} 结束原因",
        adapter.protocol()
    );
}

#[test]
/// 三条通道都构造出带鉴权、流式、含工具定义的 POST 请求。
fn builds_authenticated_streaming_requests() {
    let client = reqwest::Client::new();
    for case in cases() {
        let wire = case
            .adapter
            .build(&client, &case.route, &request(), &case.credential)
            .expect("构造请求失败");
        assert_eq!(wire.method(), reqwest::Method::POST);
        let body: Value = serde_json::from_slice(
            wire.body()
                .and_then(reqwest::Body::as_bytes)
                .expect("请求缺少 Body"),
        )
        .expect("请求 Body 不是 JSON");
        assert_eq!(body["stream"], true, "{} 必须流式", case.adapter.protocol());
        let tools = body["tools"].as_array().expect("tools 必须是数组");
        assert_eq!(tools.len(), 1, "{} 工具数", case.adapter.protocol());
        assert!(
            wire.headers().contains_key("authorization")
                || wire.headers().contains_key("x-api-key"),
            "{} 缺少鉴权头",
            case.adapter.protocol()
        );
    }
}

#[test]
/// 同一工具调用在三条流式通道上折叠出等价结果。
fn translates_tool_calls_equivalently() {
    for case in cases() {
        let mut assembler = BlockAssembler::default();
        for event in &case.stream_events {
            for piece in case.adapter.translate(event) {
                assembler.push(piece);
            }
        }
        assert_call(case.adapter, &assembler, "SSE");
    }
}

#[test]
/// 同一工具调用在三条非流式通道上折叠出等价结果。
fn translates_json_bodies_equivalently() {
    for case in cases() {
        let mut assembler = BlockAssembler::default();
        for piece in case.adapter.translate_json(&case.json_body) {
            assembler.push(piece);
        }
        assert_call(case.adapter, &assembler, "JSON");
    }
}

#[test]
/// 三条通道的过载错误都归一为可重试的 OVERLOADED。
fn classifies_overload_errors_per_protocol() {
    let cases: [(&dyn LlmAdapter, Value); 3] = [
        (
            &Chat,
            json!({"error":{"code":"server_is_overloaded","type":"service_unavailable_error"}}),
        ),
        (
            &Anthropic,
            json!({"type":"error","error":{"type":"overloaded_error","message":"overloaded"}}),
        ),
        (
            &Responses,
            json!({"type":"response.failed","response":{"error":{"code":"server_is_overloaded"}}}),
        ),
    ];
    for (adapter, event) in cases {
        let chunks = adapter.translate(&event);
        match chunks.first() {
            Some(crate::ai::llm::chunk::StreamChunk::Finish {
                failure: Some(failure),
                ..
            }) => {
                assert_eq!(
                    failure.code,
                    FailureCode::Overloaded,
                    "{}",
                    adapter.protocol()
                );
                assert!(failure.code.retryable());
            }
            other => panic!("{} 预期失败终止，得到 {other:?}", adapter.protocol()),
        }
    }
}
