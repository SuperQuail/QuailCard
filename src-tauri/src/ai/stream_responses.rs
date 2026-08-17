use std::collections::BTreeMap;

use serde_json::Value;

use super::super::ToolCallResult;
use super::{
    debug_stage, incomplete_stream, invalid_stream_response, missing_responses_call_id,
    parse_pending_call, sse_data_blocks, stream_error, tool_not_called, PendingCall,
};
use crate::error::CommandError;

/// 解析 OpenAI Responses SSE 中的全部函数工具调用。
pub(crate) fn parse_openai_responses_stream(
    body: &[u8],
) -> Result<Vec<ToolCallResult>, CommandError> {
    let mut calls = BTreeMap::<String, PendingCall>::new();
    let mut order = Vec::<String>::new();
    let mut completed = false;
    for data in sse_data_blocks(body)? {
        if data == "[DONE]" {
            continue;
        }
        let event: Value = serde_json::from_str(&data).map_err(|_| invalid_stream_response())?;
        let kind = event
            .get("type")
            .and_then(Value::as_str)
            .unwrap_or_default();
        match kind {
            "response.output_item.added"
                if event.pointer("/item/type").and_then(Value::as_str) == Some("function_call") =>
            {
                if let Some(id) = event.pointer("/item/id").and_then(Value::as_str) {
                    let call = calls.entry(id.to_string()).or_default();
                    call.item_id = Some(id.to_string());
                    call.id = event
                        .pointer("/item/call_id")
                        .and_then(Value::as_str)
                        .map(str::to_string);
                    call.name = event
                        .pointer("/item/name")
                        .and_then(Value::as_str)
                        .unwrap_or_default()
                        .to_string();
                    call.arguments = event
                        .pointer("/item/arguments")
                        .and_then(Value::as_str)
                        .unwrap_or_default()
                        .to_string();
                    if !order.iter().any(|item| item == id) {
                        order.push(id.to_string());
                    }
                }
            }
            "response.function_call_arguments.delta" => {
                if let (Some(id), Some(delta)) = (
                    event.get("item_id").and_then(Value::as_str),
                    event.get("delta").and_then(Value::as_str),
                ) {
                    calls
                        .entry(id.to_string())
                        .or_default()
                        .arguments
                        .push_str(delta);
                }
            }
            "response.function_call_arguments.done" => {
                if let Some(id) = event.get("item_id").and_then(Value::as_str) {
                    let call = calls.entry(id.to_string()).or_default();
                    call.item_id = Some(id.to_string());
                    if let Some(name) = event.get("name").and_then(Value::as_str) {
                        call.name = name.to_string();
                    }
                    if let Some(arguments) = event.get("arguments").and_then(Value::as_str) {
                        call.arguments = arguments.to_string();
                    }
                    if !order.iter().any(|item| item == id) {
                        order.push(id.to_string());
                    }
                }
            }
            "response.output_item.done"
                if event.pointer("/item/type").and_then(Value::as_str) == Some("function_call") =>
            {
                if let Some(id) = event.pointer("/item/id").and_then(Value::as_str) {
                    let call = calls.entry(id.to_string()).or_default();
                    call.item_id = Some(id.to_string());
                    call.id = event
                        .pointer("/item/call_id")
                        .and_then(Value::as_str)
                        .map(str::to_string)
                        .or_else(|| call.id.clone());
                    if let Some(name) = event.pointer("/item/name").and_then(Value::as_str) {
                        call.name = name.to_string();
                    }
                    if let Some(arguments) =
                        event.pointer("/item/arguments").and_then(Value::as_str)
                    {
                        call.arguments = arguments.to_string();
                    }
                    if !order.iter().any(|item| item == id) {
                        order.push(id.to_string());
                    }
                }
            }
            "response.completed" => {
                completed = true;
                if let Some(output) = event.pointer("/response/output").and_then(Value::as_array) {
                    for item in output.iter().filter(|item| {
                        item.get("type").and_then(Value::as_str) == Some("function_call")
                    }) {
                        let Some(id) = item.get("id").and_then(Value::as_str) else {
                            continue;
                        };
                        let call = calls.entry(id.to_string()).or_default();
                        call.item_id = Some(id.to_string());
                        call.id = item
                            .get("call_id")
                            .and_then(Value::as_str)
                            .map(str::to_string);
                        call.name = item
                            .get("name")
                            .and_then(Value::as_str)
                            .unwrap_or_default()
                            .to_string();
                        call.arguments = item
                            .get("arguments")
                            .and_then(Value::as_str)
                            .unwrap_or_default()
                            .to_string();
                        if !order.iter().any(|value| value == id) {
                            order.push(id.to_string());
                        }
                    }
                }
            }
            "response.incomplete" => return Err(incomplete_stream()),
            "response.failed" | "error" => return Err(stream_error(&event)),
            _ => {}
        }
    }
    if !completed {
        return Err(incomplete_stream());
    }
    if calls.is_empty() {
        return Err(tool_not_called());
    }
    if calls
        .values()
        .any(|call| call.id.is_none() || call.item_id.is_none())
    {
        return Err(missing_responses_call_id());
    }
    order
        .into_iter()
        .filter_map(|id| calls.remove(&id).map(|call| (id, call)))
        .enumerate()
        .map(|(index, (_id, call))| {
            let call_id = call.id.clone();
            Ok(parse_pending_call(index, call_id, call))
        })
        .collect()
}

/// 提取 Responses SSE 完整输出项，供 store=false 的下一轮无状态续传。
pub(crate) fn openai_responses_output_items(body: &[u8]) -> Result<Vec<Value>, CommandError> {
    let mut completed_output = None;
    let mut done_items = Vec::new();
    for data in sse_data_blocks(body)? {
        if data == "[DONE]" {
            continue;
        }
        let event: Value = serde_json::from_str(&data).map_err(|_| invalid_stream_response())?;
        match event.get("type").and_then(Value::as_str) {
            Some("response.output_item.done") => {
                if let Some(item) = event.get("item") {
                    done_items.push(item.clone());
                }
            }
            Some("response.completed") => {
                completed_output = event
                    .pointer("/response/output")
                    .and_then(Value::as_array)
                    .cloned();
            }
            _ => {}
        }
    }
    Ok(completed_output
        .filter(|items| !items.is_empty())
        .unwrap_or(done_items))
}

/// 打印 OpenAI Responses 流中的事件阶段和内容增量。
pub(super) fn log_responses_event(trace_id: &str, event: &Value) {
    let kind = event.get("type").and_then(Value::as_str).unwrap_or("event");
    if kind == "response.output_item.added" {
        let item_type = event
            .pointer("/item/type")
            .and_then(Value::as_str)
            .unwrap_or("unknown");
        let name = event
            .pointer("/item/name")
            .and_then(Value::as_str)
            .unwrap_or("");
        debug_stage(trace_id, format!("item.start: {item_type} {name}"));
    } else if matches!(kind, "response.completed" | "response.incomplete" | "error") {
        debug_stage(trace_id, kind);
    }
}
