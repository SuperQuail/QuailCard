//! 运行时折叠与错误映射回归测试。

use super::super::failure::{FailureCode, LlmFailure};
use super::*;

#[test]
/// 失败码映射到既有 PROVIDER_* 契约，前端无需改动。
fn maps_failure_codes_to_existing_contract() {
    let timeout = command_error(LlmFailure::provider(FailureCode::Timeout, "超时"));
    assert_eq!(timeout.code, "PROVIDER_TIMEOUT");
    let overloaded = command_error(LlmFailure::provider(FailureCode::Overloaded, "过载"));
    assert_eq!(overloaded.code, "PROVIDER_OVERLOADED");
    let missing = command_error(LlmFailure::provider(
        FailureCode::MissingCredential,
        "缺少凭据",
    ));
    assert_eq!(missing.code, "PROVIDER_CREDENTIAL_MISSING");
}

#[test]
/// 协议枚举与注册表标签一一对应。
fn protocol_names_match_registry_keys() {
    assert_eq!(
        protocol_name(ProviderProtocol::OpenAiCompatible),
        "OpenAI Compatible"
    );
    assert_eq!(
        protocol_name(ProviderProtocol::AnthropicMessages),
        "Anthropic Messages"
    );
}

#[test]
/// 可见文本与思考各走自己的回调；工具参数不外发，未提供回调时静默跳过。
fn routes_stream_deltas_to_their_own_sink() {
    let text = std::sync::Mutex::new(Vec::new());
    let reasoning = std::sync::Mutex::new(Vec::new());
    let on_text = |value: &str| text.lock().unwrap().push(value.to_string());
    let on_reasoning = |value: &str| reasoning.lock().unwrap().push(value.to_string());
    let sinks = StreamSinks {
        text: Some(&on_text),
        reasoning: Some(&on_reasoning),
    };
    forward_stream(
        &StreamChunk::TextDelta {
            index: 1,
            text: "你好".into(),
        },
        sinks,
    );
    forward_stream(
        &StreamChunk::ReasoningDelta {
            index: 0,
            text: "私有思考".into(),
        },
        sinks,
    );
    forward_stream(
        &StreamChunk::ToolCallDelta {
            index: 2,
            id: "call_1".into(),
            item_id: None,
            name: Some("emit".into()),
            arguments_delta: "{}".into(),
        },
        sinks,
    );
    assert_eq!(*text.lock().unwrap(), ["你好"]);
    assert_eq!(*reasoning.lock().unwrap(), ["私有思考"]);
}
