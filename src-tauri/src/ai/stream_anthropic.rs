use std::collections::BTreeMap;

use serde_json::Value;

use super::super::ToolCallResult;
use super::{
    debug_delta, debug_stage, finish_calls, incomplete_stream, invalid_stream_response,
    sse_data_blocks, stream_error, PendingCall,
};
use crate::error::CommandError;

/// 解析 Anthropic Messages SSE 中的全部 tool_use 内容块。
pub(crate) fn parse_anthropic_stream(body: &[u8]) -> Result<Vec<ToolCallResult>, CommandError> {
    let mut calls = BTreeMap::<usize, PendingCall>::new();
    let mut completed = false;
    for data in sse_data_blocks(body)? {
        let event: Value = serde_json::from_str(&data).map_err(|_| invalid_stream_response())?;
        let kind = event
            .get("type")
            .and_then(Value::as_str)
            .unwrap_or_default();
        let index = event.get("index").and_then(Value::as_u64).unwrap_or(0) as usize;
        match kind {
            "content_block_start"
                if event.pointer("/content_block/type").and_then(Value::as_str)
                    == Some("tool_use") =>
            {
                let block = &event["content_block"];
                let call = calls.entry(index).or_default();
                call.id = block.get("id").and_then(Value::as_str).map(str::to_string);
                call.name = block
                    .get("name")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_string();
                call.initial_arguments = block.get("input").and_then(|input| {
                    (!input.is_null() && input.as_object().is_none_or(|value| !value.is_empty()))
                        .then(|| input.to_string())
                });
            }
            "content_block_delta"
                if event.pointer("/delta/type").and_then(Value::as_str)
                    == Some("input_json_delta") =>
            {
                if let Some(partial) = event.pointer("/delta/partial_json").and_then(Value::as_str)
                {
                    calls.entry(index).or_default().arguments.push_str(partial);
                }
            }
            "message_stop" => completed = true,
            "error" => return Err(stream_error(&event)),
            _ => {}
        }
    }
    if !completed {
        return Err(incomplete_stream());
    }
    finish_calls(calls)
}

/// 打印 Anthropic 流中的思考、文字与工具参数增量。
pub(super) fn log_anthropic_event(trace_id: &str, event: &Value) {
    let kind = event
        .get("type")
        .and_then(Value::as_str)
        .unwrap_or_default();
    match kind {
        "content_block_start" => {
            let block_type = event
                .pointer("/content_block/type")
                .and_then(Value::as_str)
                .unwrap_or("unknown");
            let name = event
                .pointer("/content_block/name")
                .and_then(Value::as_str)
                .unwrap_or("");
            debug_stage(trace_id, format!("content.start: {block_type} {name}"));
        }
        "content_block_delta" => {
            let delta = &event["delta"];
            let delta_type = delta.get("type").and_then(Value::as_str).unwrap_or("delta");
            let content = delta
                .get("thinking")
                .or_else(|| delta.get("text"))
                .or_else(|| delta.get("partial_json"))
                .and_then(Value::as_str)
                .unwrap_or("");
            if !content.is_empty() {
                debug_delta(trace_id, delta_type, content);
            }
        }
        "message_stop" | "error" => debug_stage(trace_id, kind),
        _ => {}
    }
}
