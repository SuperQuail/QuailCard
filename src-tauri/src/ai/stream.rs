use std::collections::BTreeMap;

use reqwest::Response;
use serde_json::Value;

use super::ToolCallResult;
use crate::error::CommandError;

#[path = "stream_anthropic.rs"]
mod stream_anthropic;
#[path = "stream_openai.rs"]
mod stream_openai;
#[path = "stream_responses.rs"]
mod stream_responses;

use stream_anthropic::log_anthropic_event;
use stream_openai::log_openai_chat_event;
use stream_responses::log_responses_event;

pub(super) use stream_anthropic::parse_anthropic_stream;
pub(super) use stream_openai::parse_openai_chat_stream;
pub(super) use stream_responses::{openai_responses_output_items, parse_openai_responses_stream};

const MAX_RESPONSE_BYTES: usize = 5 * 1024 * 1024;

/// 控制实时日志解析所采用的供应商流协议。
#[derive(Clone, Copy)]
pub(super) enum StreamProtocol {
    OpenAiChat,
    Anthropic,
    OpenAiResponses,
}

#[derive(Default)]
struct PendingCall {
    pub(super) id: Option<String>,
    pub(super) item_id: Option<String>,
    pub(super) name: String,
    pub(super) arguments: String,
    pub(super) initial_arguments: Option<String>,
}

/// 逐块读取供应商响应，在 Debug 控制台实时打印完整 SSE 事件中的增量内容。
pub(super) async fn read_model_body(
    mut response: Response,
    protocol: StreamProtocol,
    trace_id: &str,
) -> Result<Vec<u8>, CommandError> {
    let is_event_stream = response
        .headers()
        .get(reqwest::header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .is_some_and(|value| value.contains("text/event-stream"));
    let mut body = Vec::new();
    let mut pending = Vec::new();
    while let Some(chunk) = response.chunk().await.map_err(map_stream_error)? {
        if body.len() + chunk.len() > MAX_RESPONSE_BYTES {
            return Err(CommandError::provider(
                "PROVIDER_RESPONSE_TOO_LARGE",
                "模型响应超过 5 MiB 限制",
            ));
        }
        body.extend_from_slice(&chunk);
        if is_event_stream {
            pending.extend_from_slice(&chunk);
            log_complete_events(&mut pending, protocol, trace_id)?;
        }
    }
    if is_event_stream && !pending.is_empty() {
        log_sse_block(&pending, protocol, trace_id)?;
    }
    if !is_event_stream {
        debug_stage(trace_id, "response.complete: non-stream JSON");
    }
    Ok(body)
}

/// 在 Debug 构建中输出 AI 调用阶段日志。
pub(crate) fn debug_stage(trace_id: &str, message: impl AsRef<str>) {
    #[cfg(debug_assertions)]
    eprintln!("[QuailCard][AI trace={trace_id}] {}", message.as_ref());
    #[cfg(not(debug_assertions))]
    let _ = (trace_id, message);
}

/// 从待处理缓冲区中提取并实时记录全部完整 SSE 事件。
fn log_complete_events(
    pending: &mut Vec<u8>,
    protocol: StreamProtocol,
    trace_id: &str,
) -> Result<(), CommandError> {
    while let Some((index, delimiter_len)) = find_event_delimiter(pending) {
        let block = pending[..index].to_vec();
        pending.drain(..index + delimiter_len);
        log_sse_block(&block, protocol, trace_id)?;
    }
    Ok(())
}

/// 解析并打印一个完整 SSE 事件中的可读增量。
fn log_sse_block(
    block: &[u8],
    protocol: StreamProtocol,
    trace_id: &str,
) -> Result<(), CommandError> {
    let Some(data) = extract_sse_data(block)? else {
        return Ok(());
    };
    if data == "[DONE]" {
        debug_stage(trace_id, "stream.done");
        return Ok(());
    }
    let Ok(event) = serde_json::from_str::<Value>(&data) else {
        return Ok(());
    };
    match protocol {
        StreamProtocol::OpenAiChat => log_openai_chat_event(trace_id, &event),
        StreamProtocol::Anthropic => log_anthropic_event(trace_id, &event),
        StreamProtocol::OpenAiResponses => log_responses_event(trace_id, &event),
    }
    Ok(())
}

/// 将按顺序累积的工具调用转换为统一结果。
fn finish_calls(calls: BTreeMap<usize, PendingCall>) -> Result<Vec<ToolCallResult>, CommandError> {
    if calls.is_empty() {
        return Err(tool_not_called());
    }
    calls
        .into_iter()
        .enumerate()
        .map(|(fallback_index, (index, call))| {
            let id = call.id.clone().or_else(|| Some(format!("call_{index}")));
            parse_pending_call(fallback_index, id, call)
        })
        .collect()
}

/// 解析一个已累积完成的工具调用参数。
fn parse_pending_call(
    index: usize,
    id: Option<String>,
    call: PendingCall,
) -> Result<ToolCallResult, CommandError> {
    let arguments = if call.arguments.trim().is_empty() {
        call.initial_arguments.unwrap_or_else(|| "{}".to_string())
    } else {
        call.arguments
    };
    let arguments = serde_json::from_str(&arguments).map_err(|_| invalid_tool_arguments())?;
    Ok(ToolCallResult {
        id: id.unwrap_or_else(|| format!("call_{index}")),
        item_id: call.item_id,
        name: call.name,
        arguments,
    })
}

/// 从完整响应正文中提取 SSE data 字段。
fn sse_data_blocks(body: &[u8]) -> Result<Vec<String>, CommandError> {
    let text = std::str::from_utf8(body).map_err(|_| invalid_stream_response())?;
    text.replace("\r\n", "\n")
        .split("\n\n")
        .filter_map(|block| extract_sse_data(block.as_bytes()).transpose())
        .collect()
}

/// 提取一个 SSE 事件块的多行 data 内容。
fn extract_sse_data(block: &[u8]) -> Result<Option<String>, CommandError> {
    let text = std::str::from_utf8(block).map_err(|_| invalid_stream_response())?;
    let data = text
        .lines()
        .filter_map(|line| line.strip_prefix("data:").map(str::trim_start))
        .collect::<Vec<_>>()
        .join("\n");
    Ok((!data.is_empty()).then_some(data))
}

/// 查找待处理字节中的下一个 SSE 事件分隔符。
fn find_event_delimiter(bytes: &[u8]) -> Option<(usize, usize)> {
    let lf = bytes
        .windows(2)
        .position(|window| window == b"\n\n")
        .map(|index| (index, 2));
    let crlf = bytes
        .windows(4)
        .position(|window| window == b"\r\n\r\n")
        .map(|index| (index, 4));
    match (lf, crlf) {
        (Some(left), Some(right)) => Some(if left.0 <= right.0 { left } else { right }),
        (Some(value), None) | (None, Some(value)) => Some(value),
        (None, None) => None,
    }
}

/// 将流读取网络错误转换为统一供应商错误。
fn map_stream_error(error: reqwest::Error) -> CommandError {
    if error.is_timeout() {
        CommandError::provider("PROVIDER_TIMEOUT", "模型请求超时，请检查网络或稍后重试")
    } else {
        CommandError::provider("PROVIDER_REQUEST_FAILED", "读取模型流式响应失败")
    }
}

/// 将供应商流错误事件转换为统一错误。
fn stream_error(event: &Value) -> CommandError {
    let error = event
        .get("error")
        .or_else(|| event.pointer("/response/error"));
    let code = error
        .and_then(|value| value.get("code"))
        .and_then(Value::as_str);
    let code = code.or_else(|| event.get("code").and_then(Value::as_str));
    let kind = error
        .and_then(|value| value.get("type"))
        .and_then(Value::as_str);
    let message = error
        .and_then(|value| value.get("message"))
        .and_then(Value::as_str)
        .or_else(|| event.get("message").and_then(Value::as_str))
        .unwrap_or("供应商流返回失败事件");
    if code == Some("server_is_overloaded") || kind == Some("service_unavailable_error") {
        CommandError::provider(
            "PROVIDER_OVERLOADED",
            "ChatGPT Codex 当前负载过高，请稍后重试",
        )
    } else {
        CommandError::provider("PROVIDER_REQUEST_FAILED", message)
    }
}

/// 创建模型正常结束但没有调用工具的可重试错误。
fn tool_not_called() -> CommandError {
    CommandError::provider("PROVIDER_TOOL_NOT_CALLED", "模型响应结束但没有调用工具")
}

/// 创建工具参数不是有效 JSON 的错误。
fn invalid_tool_arguments() -> CommandError {
    CommandError::provider(
        "PROVIDER_TOOL_RESPONSE_INVALID",
        "模型返回的工具参数不是有效 JSON",
    )
}

/// 创建流式响应结构无法识别的错误。
fn invalid_stream_response() -> CommandError {
    CommandError::provider(
        "PROVIDER_RESPONSE_INVALID",
        "供应商返回了无法识别的流式响应",
    )
}

/// 创建供应商流未发送正常终止事件的错误。
fn incomplete_stream() -> CommandError {
    CommandError::provider(
        "PROVIDER_RESPONSE_INCOMPLETE",
        "供应商流式响应在正常结束前中断",
    )
}

/// 创建 Responses 函数调用缺少 item_id 或 call_id 的结构错误。
fn missing_responses_call_id() -> CommandError {
    CommandError::provider(
        "PROVIDER_RESPONSE_INVALID",
        "ChatGPT Codex 函数调用缺少 item_id 或 call_id",
    )
}

/// 截断过长的失败事件日志，避免控制台刷屏。
fn truncate_log(text: String) -> String {
    const MAX_LOG_CHARS: usize = 2_000;
    if text.chars().count() <= MAX_LOG_CHARS {
        text
    } else {
        let mut truncated = text.chars().take(MAX_LOG_CHARS).collect::<String>();
        truncated.push('…');
        truncated
    }
}

#[cfg(test)]
#[path = "stream_tests.rs"]
mod tests;
