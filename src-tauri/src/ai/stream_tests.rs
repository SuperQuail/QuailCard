use super::*;

#[test]
/// OpenAI Chat 流能够跨事件拼接工具参数。
fn parses_openai_chat_tool_deltas() {
    let body = concat!(
        "data: {\"choices\":[{\"delta\":{\"tool_calls\":[{\"index\":0,\"id\":\"call_1\",\"function\":{\"name\":\"lookup_words\",\"arguments\":\"{\\\"words\\\":[\"}}]}}]}\n\n",
        "data: {\"choices\":[{\"delta\":{\"tool_calls\":[{\"index\":0,\"function\":{\"arguments\":\"\\\"speak\\\"]}\"}}]},\"finish_reason\":\"tool_calls\"}]}\n\n",
        "data: [DONE]\n\n"
    );
    let calls = parse_openai_chat_stream(body.as_bytes()).expect("解析 OpenAI 流失败");
    assert_eq!(calls[0].name, "lookup_words");
    assert_eq!(calls[0].arguments.valid().unwrap()["words"][0], "speak");
}

#[test]
/// Anthropic 流能够拼接 input_json_delta 工具参数。
fn parses_anthropic_tool_deltas() {
    let body = concat!(
        "event: content_block_start\ndata: {\"type\":\"content_block_start\",\"index\":0,\"content_block\":{\"type\":\"tool_use\",\"id\":\"toolu_1\",\"name\":\"emit_card\",\"input\":{}}}\n\n",
        "event: content_block_delta\ndata: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"input_json_delta\",\"partial_json\":\"{\\\"schema_version\\\":1}\"}}\n\n",
        "event: message_stop\ndata: {\"type\":\"message_stop\"}\n\n"
    );
    let calls = parse_anthropic_stream(body.as_bytes()).expect("解析 Anthropic 流失败");
    assert_eq!(calls[0].name, "emit_card");
    assert_eq!(calls[0].arguments.valid().unwrap()["schema_version"], 1);
}

#[test]
/// 流中一个无效调用不会丢弃同轮合法调用。
fn preserves_valid_stream_call_beside_malformed_call() {
    let body = concat!(
        "data: {\"choices\":[{\"delta\":{\"tool_calls\":[{\"index\":0,\"id\":\"bad\",\"function\":{\"name\":\"emit_card\",\"arguments\":\"{\"}},{\"index\":1,\"id\":\"good\",\"function\":{\"name\":\"emit_card\",\"arguments\":\"{}\"}}]},\"finish_reason\":\"tool_calls\"}]}\n\n",
        "data: [DONE]\n\n"
    );
    let calls = parse_openai_chat_stream(body.as_bytes()).expect("解析多调用流失败");
    assert!(matches!(calls[0].arguments, ToolArguments::Invalid(_)));
    assert!(matches!(calls[1].arguments, ToolArguments::Valid(_)));
}

#[test]
/// 正常结束但没有工具调用时返回独立可重试错误码。
fn identifies_missing_tool_call() {
    let body = "data: {\"choices\":[{\"delta\":{\"content\":\"hello\"},\"finish_reason\":\"stop\"}]}\n\ndata: [DONE]\n\n";
    let error = parse_openai_chat_stream(body.as_bytes()).unwrap_err();
    assert_eq!(error.code, "PROVIDER_TOOL_NOT_CALLED");
}

#[test]
/// 缺少终止事件的截断流不会被误判为可重试的工具缺失。
fn rejects_truncated_stream() {
    let body = "data: {\"choices\":[{\"delta\":{\"content\":\"partial\"}}]}\n\n";
    let error = parse_openai_chat_stream(body.as_bytes()).unwrap_err();
    assert_eq!(error.code, "PROVIDER_RESPONSE_INCOMPLETE");
}

#[test]
/// Responses 顶层错误字段会保留过载分类和真实消息。
fn parses_top_level_responses_error() {
    let body = "data: {\"type\":\"error\",\"code\":\"server_is_overloaded\",\"message\":\"overloaded\"}\n\n";
    let error = parse_openai_responses_stream(body.as_bytes()).unwrap_err();
    assert_eq!(error.code, "PROVIDER_OVERLOADED");
}

#[test]
/// SSE 分隔符检测同时支持 LF 和 CRLF 且允许跨网络块累积。
fn finds_sse_event_delimiters() {
    assert_eq!(find_event_delimiter(b"data: x\n\nnext"), Some((7, 2)));
    assert_eq!(find_event_delimiter(b"data: x\r\n\r\nnext"), Some((7, 4)));
    assert_eq!(find_event_delimiter(b"data: x\r\n"), None);
}
