use std::collections::BTreeMap;

use serde_json::Value;

use super::super::ToolCallResult;
use super::{
    debug_delta, debug_stage, finish_calls, incomplete_stream, invalid_stream_response,
    sse_data_blocks, stream_error, PendingCall,
};
use crate::error::CommandError;

/// 解析 OpenAI Chat Completions SSE 中的全部工具调用。
pub(crate) fn parse_openai_chat_stream(body: &[u8]) -> Result<Vec<ToolCallResult>, CommandError> {
    let mut calls = BTreeMap::<usize, PendingCall>::new();
    let mut completed = false;
    for data in sse_data_blocks(body)? {
        if data == "[DONE]" {
            completed = true;
            continue;
        }
        let event: Value = serde_json::from_str(&data).map_err(|_| invalid_stream_response())?;
        if event.get("error").is_some() {
            return Err(stream_error(&event));
        }
        let Some(choices) = event.get("choices").and_then(Value::as_array) else {
            continue;
        };
        for choice in choices {
            if choice
                .get("finish_reason")
                .is_some_and(|reason| !reason.is_null())
            {
                completed = true;
            }
            let Some(tool_calls) = choice
                .get("delta")
                .and_then(|delta| delta.get("tool_calls"))
                .and_then(Value::as_array)
            else {
                continue;
            };
            for tool_call in tool_calls {
                let index = tool_call
                    .get("index")
                    .and_then(Value::as_u64)
                    .unwrap_or(calls.len() as u64) as usize;
                let call = calls.entry(index).or_default();
                if let Some(id) = tool_call.get("id").and_then(Value::as_str) {
                    call.id = Some(id.to_string());
                }
                if let Some(name) = tool_call
                    .get("function")
                    .and_then(|function| function.get("name"))
                    .and_then(Value::as_str)
                {
                    call.name.push_str(name);
                }
                if let Some(arguments) = tool_call
                    .get("function")
                    .and_then(|function| function.get("arguments"))
                    .and_then(Value::as_str)
                {
                    call.arguments.push_str(arguments);
                }
            }
        }
    }
    if !completed {
        return Err(incomplete_stream());
    }
    finish_calls(calls)
}

/// 打印 OpenAI Chat 流中的文字、思考与工具参数增量。
pub(super) fn log_openai_chat_event(trace_id: &str, event: &Value) {
    let Some(choices) = event.get("choices").and_then(Value::as_array) else {
        return;
    };
    for choice in choices {
        let delta = &choice["delta"];
        for key in ["reasoning_content", "reasoning", "content"] {
            if let Some(content) = delta.get(key).and_then(Value::as_str) {
                if !content.is_empty() {
                    debug_delta(trace_id, key, content);
                }
            }
        }
        if let Some(tool_calls) = delta.get("tool_calls").and_then(Value::as_array) {
            for call in tool_calls {
                if let Some(name) = call.pointer("/function/name").and_then(Value::as_str) {
                    debug_stage(trace_id, format!("tool.start: {name}"));
                }
            }
        }
        if let Some(reason) = choice.get("finish_reason").and_then(Value::as_str) {
            debug_stage(trace_id, format!("stream.finish: {reason}"));
        }
    }
}
