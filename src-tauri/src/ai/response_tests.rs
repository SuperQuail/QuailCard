use super::*;

#[test]
/// OpenAI 文本回答不能冒充工具调用结果。
fn rejects_openai_text_without_tool_call() {
    let body = br#"{"choices":[{"message":{"content":"{\"value\":1}","tool_calls":[]}}]}"#;
    let error = parse_openai_tool_response(body, "emit_result").unwrap_err();
    assert_eq!(error.code, "PROVIDER_TOOL_NOT_CALLED");
}

#[test]
/// OpenAI 只解析指定函数的 arguments。
fn parses_openai_tool_arguments() {
    let body = br#"{"choices":[{"message":{"tool_calls":[{"type":"function","function":{"name":"emit_result","arguments":"{\"value\":1}"}}]}}]}"#;
    let value = parse_openai_tool_response(body, "emit_result").expect("解析工具参数失败");
    assert_eq!(value["value"], 1);
}

#[test]
/// 单工具响应调用了其他工具时不会按“未调用工具”重试。
fn rejects_unexpected_tool_without_missing_call_error() {
    let body = br#"{"choices":[{"message":{"tool_calls":[{"type":"function","function":{"name":"other_tool","arguments":"{}"}}]}}]}"#;
    let error = parse_openai_tool_response(body, "emit_result").unwrap_err();
    assert_eq!(error.code, "PROVIDER_TOOL_RESPONSE_INVALID");
}

#[test]
/// OpenAI Responses 只解析 output 中指定的 function_call。
fn parses_openai_responses_arguments() {
    let body = br#"{"output":[{"type":"reasoning"},{"id":"fc_1","call_id":"call_1","type":"function_call","name":"emit_result","arguments":"{\"value\":1}"}]}"#;
    let value = parse_openai_responses_body(body, "emit_result").expect("解析工具参数失败");
    assert_eq!(value["value"], 1);
}

#[test]
/// Codex SSE 使用 output_item.done 中的完整参数覆盖增量。
fn parses_openai_responses_stream_arguments() {
    let body = concat!(
        "data: {\"type\":\"response.output_item.added\",\"item\":{\"id\":\"fc_1\",\"call_id\":\"call_1\",\"type\":\"function_call\",\"name\":\"emit_result\",\"arguments\":\"\"}}\n\n",
        "data: {\"type\":\"response.function_call_arguments.delta\",\"item_id\":\"fc_1\",\"delta\":\"{\\\"value\\\":\"}\n\n",
        "data: {\"type\":\"response.output_item.done\",\"item\":{\"id\":\"fc_1\",\"call_id\":\"call_1\",\"type\":\"function_call\",\"name\":\"emit_result\",\"arguments\":\"{\\\"value\\\":1}\"}}\n\n",
        "data: {\"type\":\"response.completed\",\"response\":{\"output\":[]}}\n\n",
        "data: [DONE]\n\n"
    );
    let value = parse_openai_responses_payload(body.as_bytes(), "emit_result")
        .expect("解析 SSE 工具参数失败");
    assert_eq!(value["value"], 1);
}

#[test]
/// Anthropic 只解析指定 tool_use 内容块。
fn parses_anthropic_tool_input() {
    let body = br#"{"content":[{"type":"text","text":"ignored"},{"type":"tool_use","name":"emit_result","input":{"value":1}}]}"#;
    let value = parse_anthropic_tool_response(body, "emit_result").expect("解析工具输入失败");
    assert_eq!(value["value"], 1);
}

#[test]
/// Anthropic 普通文本内容不能作为结构化结果回退。
fn rejects_anthropic_text_without_tool_use() {
    let body = br#"{"content":[{"type":"text","text":"{\"value\":1}"}]}"#;
    let error = parse_anthropic_tool_response(body, "emit_result").unwrap_err();
    assert_eq!(error.code, "PROVIDER_TOOL_NOT_CALLED");
}

#[test]
/// 供应商错误 JSON 能提取 message 用于界面提示。
fn extracts_provider_error_message() {
    let body = br#"{"error":{"code":"model_not_found","type":"invalid_request_error","message":"The model 'gpt-5.5' does not exist"}}"#;
    assert_eq!(
        provider_error_message(body).as_deref(),
        Some("The model 'gpt-5.5' does not exist")
    );
}

#[test]
/// 供应商错误摘要保留 code、type 与 message 供控制台排查。
fn summarizes_provider_error() {
    let body = br#"{"error":{"code":"model_not_found","type":"invalid_request_error","message":"The model does not exist"}}"#;
    assert_eq!(
        provider_error_summary(body),
        "code=model_not_found type=invalid_request_error message=The model does not exist"
    );
}

#[test]
/// 非 JSON 错误正文直接截断展示。
fn summarizes_plain_text_error() {
    assert_eq!(provider_error_summary(b"forbidden"), "forbidden");
    let long = format!("x{}", "y".repeat(2500));
    let summary = provider_error_summary(long.as_bytes());
    assert_eq!(summary.chars().count(), 2001);
}

#[test]
/// OpenAI Chat 解析全部函数调用并保留各自参数。
fn parses_openai_multiple_tool_calls() {
    let body = br#"{"choices":[{"message":{"tool_calls":[
        {"id":"call_1","type":"function","function":{"name":"lookup_words","arguments":"{\"words\":[\"a\"]}"}},
        {"id":"call_2","type":"function","function":{"name":"emit_cards","arguments":"{\"cards\":[]}"}}
    ]}}]}"#;
    let calls = parse_openai_tool_calls(body).expect("解析工具调用失败");
    assert_eq!(calls.len(), 2);
    assert_eq!(calls[0].id, "call_1");
    assert_eq!(calls[0].name, "lookup_words");
    assert_eq!(calls[0].arguments["words"][0], "a");
    assert_eq!(calls[1].name, "emit_cards");
}

#[test]
/// Anthropic 解析全部 tool_use 并保留调用标识。
fn parses_anthropic_multiple_tool_calls() {
    let body = br#"{"content":[
        {"type":"tool_use","id":"toolu_1","name":"lookup_words","input":{"words":["a"]}},
        {"type":"tool_use","id":"toolu_2","name":"emit_cards","input":{"cards":[]}}
    ]}"#;
    let calls = parse_anthropic_tool_calls(body).expect("解析工具调用失败");
    assert_eq!(calls.len(), 2);
    assert_eq!(calls[0].id, "toolu_1");
    assert_eq!(calls[0].name, "lookup_words");
    assert_eq!(calls[1].id, "toolu_2");
}

#[test]
/// OpenAI Responses 非流式输出解析全部函数调用。
fn parses_openai_responses_multiple_calls() {
    let body = br#"{"output":[
        {"id":"fc_1","call_id":"call_1","type":"function_call","name":"lookup_words","arguments":"{\"words\":[\"a\"]}"},
        {"id":"fc_2","call_id":"call_2","type":"function_call","name":"emit_cards","arguments":"{\"cards\":[]}"}
    ]}"#;
    let calls = parse_openai_responses_calls(body).expect("解析工具调用失败");
    assert_eq!(calls.len(), 2);
    assert_eq!(calls[0].id, "call_1");
    assert_eq!(calls[0].item_id.as_deref(), Some("fc_1"));
    assert_eq!(calls[0].name, "lookup_words");
    assert_eq!(calls[1].name, "emit_cards");
}

#[test]
/// Responses 批次保留完整 reasoning 和函数调用输出用于无状态续传。
fn preserves_responses_continuation_items() {
    let body = br#"{"output":[
        {"id":"rs_1","type":"reasoning","encrypted_content":"encrypted","summary":[]},
        {"id":"fc_1","call_id":"call_1","type":"function_call","name":"lookup_words","arguments":"{\"words\":[\"a\"]}"}
    ]}"#;
    let batch = parse_openai_responses_batch(body).expect("解析 Responses 批次失败");
    assert_eq!(batch.calls[0].id, "call_1");
    assert_eq!(batch.continuation_items.len(), 2);
    assert_eq!(
        batch.continuation_items[0]["encrypted_content"],
        "encrypted"
    );
}

#[test]
/// Responses 函数调用缺少 call_id 时拒绝构造无状态续传历史。
fn rejects_responses_call_without_call_id() {
    let body = br#"{"output":[
        {"id":"fc_1","type":"function_call","name":"lookup_words","arguments":"{\"words\":[\"a\"]}"}
    ]}"#;
    let error = parse_openai_responses_calls(body).unwrap_err();
    assert_eq!(error.code, "PROVIDER_RESPONSE_INVALID");
}

#[test]
/// Codex SSE 流中解析全部函数调用。
fn parses_openai_responses_stream_multiple_calls() {
    let body = concat!(
        "data: {\"type\":\"response.output_item.added\",\"item\":{\"id\":\"fc_1\",\"call_id\":\"call_1\",\"type\":\"function_call\",\"name\":\"lookup_words\",\"arguments\":\"\"}}\n\n",
        "data: {\"type\":\"response.function_call_arguments.delta\",\"item_id\":\"fc_1\",\"delta\":\"{\\\"words\\\":[\\\"a\\\"]}\"}\n\n",
        "data: {\"type\":\"response.output_item.done\",\"item\":{\"id\":\"fc_1\",\"call_id\":\"call_1\",\"type\":\"function_call\",\"name\":\"lookup_words\",\"arguments\":\"{\\\"words\\\":[\\\"a\\\"]}\"}}\n\n",
        "data: {\"type\":\"response.output_item.added\",\"item\":{\"id\":\"fc_2\",\"call_id\":\"call_2\",\"type\":\"function_call\",\"name\":\"emit_cards\",\"arguments\":\"{\\\"cards\\\":[]}\"}}\n\n",
        "data: {\"type\":\"response.output_item.done\",\"item\":{\"id\":\"fc_2\",\"type\":\"function_call\",\"name\":\"emit_cards\",\"arguments\":\"{\\\"cards\\\":[]}\"}}\n\n",
        "data: {\"type\":\"response.completed\",\"response\":{\"output\":[]}}\n\n",
        "data: [DONE]\n\n"
    );
    let calls = parse_openai_responses_calls(body.as_bytes()).expect("解析 SSE 调用失败");
    assert_eq!(calls.len(), 2);
    assert_eq!(calls[0].id, "call_1");
    assert_eq!(calls[0].item_id.as_deref(), Some("fc_1"));
    assert_eq!(calls[0].name, "lookup_words");
    assert_eq!(calls[0].arguments["words"][0], "a");
    assert_eq!(calls[1].name, "emit_cards");
    let batch = parse_openai_responses_batch(body.as_bytes()).expect("解析 SSE 批次失败");
    assert_eq!(batch.continuation_items[1]["call_id"], "call_2");
}

#[test]
/// Codex 过载事件会转换为可供客户端识别和重试的错误码。
fn identifies_codex_overload_event() {
    let body = concat!(
        "data: {\"type\":\"error\",\"error\":{\"type\":\"service_unavailable_error\",",
        "\"code\":\"server_is_overloaded\",\"message\":\"overloaded\"}}\n\n"
    );
    let error = parse_openai_responses_calls(body.as_bytes()).unwrap_err();
    assert_eq!(error.code, "PROVIDER_OVERLOADED");
}
