use reqwest::Response;
use serde::Deserialize;
use serde_json::Value;

use super::stream::{
    openai_responses_output_items, parse_anthropic_stream, parse_openai_chat_stream,
    parse_openai_responses_stream as parse_openai_responses_calls_sse, read_model_body,
    StreamProtocol,
};
use super::{ProviderProtocol, ToolArgumentError, ToolArguments, ToolCallBatch, ToolCallResult};
use crate::{error::CommandError, models::ProviderConfig};

const MAX_RESPONSE_BYTES: usize = 5 * 1024 * 1024;

#[derive(Deserialize)]
struct OpenAiResponse {
    choices: Vec<OpenAiChoice>,
}

#[derive(Deserialize)]
struct OpenAiChoice {
    message: OpenAiMessage,
}

#[derive(Deserialize)]
struct OpenAiMessage {
    #[serde(default)]
    tool_calls: Vec<OpenAiToolCall>,
}

#[derive(Deserialize)]
struct OpenAiToolCall {
    id: Option<String>,
    #[serde(rename = "type")]
    kind: String,
    function: OpenAiFunctionCall,
}

#[derive(Deserialize)]
struct OpenAiFunctionCall {
    name: String,
    arguments: String,
}

#[derive(Deserialize)]
struct OpenAiResponsesResponse {
    #[serde(default)]
    output: Vec<OpenAiResponseItem>,
}

#[derive(Deserialize)]
struct OpenAiResponseItem {
    id: Option<String>,
    call_id: Option<String>,
    #[serde(rename = "type")]
    kind: String,
    name: Option<String>,
    arguments: Option<String>,
}

#[derive(Deserialize)]
struct AnthropicResponse {
    content: Vec<AnthropicBlock>,
}

#[derive(Deserialize)]
struct AnthropicBlock {
    id: Option<String>,
    #[serde(rename = "type")]
    kind: String,
    name: Option<String>,
    input: Option<Value>,
}

/// 读取有大小上限的响应并提取指定工具参数。
pub(super) async fn parse_tool_response(
    config: &ProviderConfig,
    response: Response,
    expected_tool: &str,
    trace_id: &str,
) -> Result<Value, CommandError> {
    let status = response.status();
    if !status.is_success() {
        return Err(map_body_error(status.as_u16(), response).await);
    }
    let protocol = ProviderProtocol::parse(&config.protocol)?;
    let stream_protocol = match protocol {
        ProviderProtocol::OpenAiCompatible => StreamProtocol::OpenAiChat,
        ProviderProtocol::AnthropicMessages => StreamProtocol::Anthropic,
    };
    let body = read_model_body(response, stream_protocol, trace_id).await?;
    match protocol {
        ProviderProtocol::OpenAiCompatible => parse_openai_tool_response(&body, expected_tool),
        ProviderProtocol::AnthropicMessages => parse_anthropic_tool_response(&body, expected_tool),
    }
}

/// 读取 OpenAI Responses 输出并提取指定函数参数。
pub(super) async fn parse_openai_responses_tool_response(
    response: Response,
    expected_tool: &str,
    trace_id: &str,
) -> Result<Value, CommandError> {
    let status = response.status();
    if !status.is_success() {
        return Err(map_body_error(status.as_u16(), response).await);
    }
    let body = read_model_body(response, StreamProtocol::OpenAiResponses, trace_id).await?;
    parse_openai_responses_payload(&body, expected_tool)
}

/// 逐块读取响应并阻止无界内存增长。
pub(super) async fn read_limited_body(mut response: Response) -> Result<Vec<u8>, CommandError> {
    let mut body = Vec::new();
    while let Some(chunk) = response.chunk().await.map_err(map_request_error)? {
        if body.len() + chunk.len() > MAX_RESPONSE_BYTES {
            return Err(CommandError::provider(
                "PROVIDER_RESPONSE_TOO_LARGE",
                "模型响应超过 5 MiB 限制",
            ));
        }
        body.extend_from_slice(&chunk);
    }
    Ok(body)
}

/// 提取 OpenAI Compatible 响应中的指定函数参数。
fn parse_openai_tool_response(body: &[u8], expected_tool: &str) -> Result<Value, CommandError> {
    select_expected_call(parse_openai_tool_calls(body)?, expected_tool)
}

/// 提取 OpenAI Responses output 中的指定 function_call 参数。
fn parse_openai_responses_body(body: &[u8], expected_tool: &str) -> Result<Value, CommandError> {
    select_expected_call(parse_openai_responses_calls(body)?, expected_tool)
}

/// 根据响应体形态判断 Codex SSE 或标准非流式 Responses JSON。
fn parse_openai_responses_payload(body: &[u8], expected_tool: &str) -> Result<Value, CommandError> {
    if is_json_body(body) {
        return parse_openai_responses_body(body, expected_tool);
    }
    select_expected_call(parse_openai_responses_calls_sse(body)?, expected_tool)
}

/// 提取 Anthropic Messages 响应中的指定工具输入。
fn parse_anthropic_tool_response(body: &[u8], expected_tool: &str) -> Result<Value, CommandError> {
    select_expected_call(parse_anthropic_tool_calls(body)?, expected_tool)
}

/// 提取 OpenAI Compatible 响应中的全部函数调用。
pub(super) fn parse_openai_tool_calls(body: &[u8]) -> Result<Vec<ToolCallResult>, CommandError> {
    if !is_json_body(body) {
        return parse_openai_chat_stream(body);
    }
    let response: OpenAiResponse =
        serde_json::from_slice(body).map_err(|_| invalid_provider_response())?;
    let calls = response
        .choices
        .into_iter()
        .flat_map(|choice| choice.message.tool_calls)
        .filter(|call| call.kind == "function")
        .enumerate()
        .map(|(index, call)| {
            parse_call_arguments(index, call.id, call.function.name, call.function.arguments)
        })
        .collect::<Vec<_>>();
    if calls.is_empty() {
        return Err(tool_not_called());
    }
    Ok(calls)
}

/// 提取 Anthropic Messages 响应中的全部工具调用。
pub(super) fn parse_anthropic_tool_calls(body: &[u8]) -> Result<Vec<ToolCallResult>, CommandError> {
    if !is_json_body(body) {
        return parse_anthropic_stream(body);
    }
    let response: AnthropicResponse =
        serde_json::from_slice(body).map_err(|_| invalid_provider_response())?;
    let calls = response
        .content
        .into_iter()
        .filter(|block| block.kind == "tool_use")
        .enumerate()
        .map(|(index, block)| {
            parse_call_arguments(
                index,
                block.id,
                block.name.unwrap_or_default(),
                block.input.unwrap_or(Value::Null).to_string(),
            )
        })
        .collect::<Vec<_>>();
    if calls.is_empty() {
        return Err(tool_not_called());
    }
    Ok(calls)
}

/// 解析 OpenAI Responses 全部响应形态中的函数调用列表。
pub(super) fn parse_openai_responses_calls(
    body: &[u8],
) -> Result<Vec<ToolCallResult>, CommandError> {
    if is_json_body(body) {
        let response: OpenAiResponsesResponse =
            serde_json::from_slice(body).map_err(|_| invalid_provider_response())?;
        let calls = response
            .output
            .into_iter()
            .filter(|item| item.kind == "function_call")
            .enumerate()
            .map(|(index, item)| {
                let item_id = item.id.ok_or_else(missing_responses_call_id)?;
                let call_id = item.call_id.ok_or_else(missing_responses_call_id)?;
                let mut call = parse_call_arguments(
                    index,
                    Some(call_id),
                    item.name.unwrap_or_default(),
                    item.arguments.unwrap_or_default(),
                );
                call.item_id = Some(item_id);
                Ok::<ToolCallResult, CommandError>(call)
            })
            .collect::<Result<Vec<_>, _>>()?;
        if calls.is_empty() {
            return Err(tool_not_called());
        }
        return Ok(calls);
    }
    parse_openai_responses_calls_sse(body)
}

/// 解析 Responses 工具调用及无状态续传所需的完整输出项。
pub(super) fn parse_openai_responses_batch(body: &[u8]) -> Result<ToolCallBatch, CommandError> {
    let calls = parse_openai_responses_calls(body)?;
    let mut continuation_items = if is_json_body(body) {
        let response: Value =
            serde_json::from_slice(body).map_err(|_| invalid_provider_response())?;
        response
            .get("output")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default()
    } else {
        openai_responses_output_items(body)?
    };
    enrich_responses_continuation_ids(&mut continuation_items, &calls)?;
    Ok(ToolCallBatch {
        calls,
        continuation_items,
    })
}

/// 用流增量中解析出的 call_id 补齐 output_item.done，并校验每个调用都可续传。
fn enrich_responses_continuation_ids(
    items: &mut [Value],
    calls: &[ToolCallResult],
) -> Result<(), CommandError> {
    for call in calls {
        let item = items
            .iter_mut()
            .find(|item| {
                item.get("type").and_then(Value::as_str) == Some("function_call")
                    && item.get("id").and_then(Value::as_str) == call.item_id.as_deref()
            })
            .ok_or_else(missing_responses_call_id)?;
        let object = item.as_object_mut().ok_or_else(invalid_provider_response)?;
        object
            .entry("call_id")
            .or_insert_with(|| Value::String(call.id.clone()));
        if matches!(call.arguments, ToolArguments::Invalid(_)) {
            object.insert("arguments".to_string(), Value::String("{}".to_string()));
        }
    }
    Ok(())
}

/// 判断响应正文是否为普通 JSON 而不是 SSE 事件流。
fn is_json_body(body: &[u8]) -> bool {
    body.iter()
        .copied()
        .find(|byte| !byte.is_ascii_whitespace())
        == Some(b'{')
}

/// 从工具调用列表中提取指定工具的参数。
fn select_expected_call(
    calls: Vec<ToolCallResult>,
    expected_tool: &str,
) -> Result<Value, CommandError> {
    calls
        .into_iter()
        .find(|call| call.name == expected_tool)
        .map(|call| {
            call.arguments
                .valid()
                .cloned()
                .ok_or_else(invalid_tool_arguments)
        })
        .transpose()?
        .ok_or_else(|| {
            CommandError::provider(
                "PROVIDER_TOOL_RESPONSE_INVALID",
                format!("模型调用了非预期工具，必须调用 {expected_tool}"),
            )
        })
}

/// 将单个工具调用字符串参数解析为结构化结果。
fn parse_call_arguments(
    index: usize,
    id: Option<String>,
    name: String,
    arguments: String,
) -> ToolCallResult {
    let parsed = if arguments.trim().is_empty() {
        ToolArguments::Valid(Value::Object(Default::default()))
    } else {
        parse_arguments(&arguments)
    };
    ToolCallResult {
        id: id.unwrap_or_else(|| format!("call_{index}")),
        item_id: None,
        name,
        arguments: parsed,
    }
}

/// 将单次参数解析失败压缩为不含原文的安全状态。
fn parse_arguments(arguments: &str) -> ToolArguments {
    match serde_json::from_str(arguments) {
        Ok(value) => ToolArguments::Valid(value),
        Err(error) => ToolArguments::Invalid(ToolArgumentError {
            line: error.line(),
            column: error.column(),
            category: match error.classify() {
                serde_json::error::Category::Io => "io",
                serde_json::error::Category::Syntax => "syntax",
                serde_json::error::Category::Data => "data",
                serde_json::error::Category::Eof => "eof",
            },
        }),
    }
}

/// 将网络层异常转换为可操作错误。
pub(super) fn map_request_error(error: reqwest::Error) -> CommandError {
    if error.is_timeout() {
        CommandError::provider("PROVIDER_TIMEOUT", "模型请求超时，请检查网络或稍后重试")
    } else if error.is_connect() {
        CommandError::provider("PROVIDER_UNREACHABLE", "无法连接供应商地址")
    } else {
        CommandError::provider("PROVIDER_REQUEST_FAILED", "模型网络请求失败")
    }
}

/// 将 HTTP 状态码转换为固定错误码的供应商错误。
fn map_status_error(status: u16) -> CommandError {
    match status {
        401 | 403 => CommandError::provider(
            "PROVIDER_AUTH_FAILED",
            "供应商凭据无效或没有访问该模型的权限",
        ),
        404 => CommandError::provider(
            "PROVIDER_ENDPOINT_NOT_FOUND",
            "请求端点或模型不存在，请检查 BaseURL 和模型名称",
        ),
        429 => CommandError::provider("PROVIDER_RATE_LIMITED", "供应商限流，请稍后重试"),
        400 | 422 => CommandError::provider(
            "PROVIDER_TOOL_UNSUPPORTED",
            format!("供应商拒绝强制工具调用（HTTP {status}）"),
        ),
        401..=499 => CommandError::provider(
            "PROVIDER_REQUEST_REJECTED",
            format!("供应商拒绝了请求（HTTP {status}）"),
        ),
        _ => CommandError::provider(
            "PROVIDER_SERVER_ERROR",
            format!("供应商服务异常（HTTP {status}）"),
        ),
    }
}

/// 读取非 2xx 响应正文，把真实错误打印到控制台并返回带原因的供应商错误。
pub(super) async fn map_body_error(status: u16, response: Response) -> CommandError {
    let body = read_limited_body(response).await.unwrap_or_default();
    eprintln!(
        "[QuailCard] 供应商请求失败（HTTP {status}）：{}",
        provider_error_summary(&body)
    );
    let mut error = map_status_error(status);
    if let Some(message) = provider_error_message(&body) {
        error.message = format!("{}：{message}", error.message);
    }
    error
}

/// 提取供应商错误正文中的可读摘要，供控制台排查真实原因。
fn provider_error_summary(body: &[u8]) -> String {
    let text = String::from_utf8_lossy(body).trim().to_string();
    if let Ok(value) = serde_json::from_str::<Value>(&text) {
        if let Some(error) = value.get("error") {
            let code = error.get("code").and_then(Value::as_str).unwrap_or("-");
            let kind = error.get("type").and_then(Value::as_str).unwrap_or("-");
            let message = error.get("message").and_then(Value::as_str).unwrap_or("-");
            return format!("code={code} type={kind} message={message}");
        }
    }
    truncate_log(text)
}

/// 截断过长的控制台日志文本，避免输出刷屏。
fn truncate_log(text: String) -> String {
    const MAX_LOG_CHARS: usize = 2000;
    if text.chars().count() <= MAX_LOG_CHARS {
        text
    } else {
        let mut truncated: String = text.chars().take(MAX_LOG_CHARS).collect();
        truncated.push('…');
        truncated
    }
}

/// 解析供应商错误 JSON 中的 message 字段。
fn provider_error_message(body: &[u8]) -> Option<String> {
    let value: Value = serde_json::from_slice(body).ok()?;
    value
        .get("error")
        .and_then(|error| error.get("message"))
        .and_then(Value::as_str)
        .map(ToOwned::to_owned)
}

/// 创建统一的响应结构错误。
fn invalid_provider_response() -> CommandError {
    CommandError::provider("PROVIDER_RESPONSE_INVALID", "供应商返回了无法识别的响应")
}

/// 创建模型未按要求调用工具的错误。
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

/// 创建 Responses 函数调用缺少 item_id 或 call_id 的结构错误。
fn missing_responses_call_id() -> CommandError {
    CommandError::provider(
        "PROVIDER_RESPONSE_INVALID",
        "ChatGPT Codex 函数调用缺少 item_id 或 call_id",
    )
}

#[cfg(test)]
#[path = "response_tests.rs"]
mod tests;
