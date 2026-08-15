use std::time::Duration;

use reqwest::Url;
use serde_json::{json, Value};

use super::super::{debug_stage, MultiToolRequest, ProviderProtocol, ToolDefinition, ToolRequest};
use crate::{error::CommandError, models::ProviderConfig};

pub(super) const CODEX_MAX_ATTEMPTS: usize = 3;
pub(super) const MAX_MISSING_TOOL_RETRIES: usize = 3;

/// 创建连接测试复用的严格函数工具。
pub(super) fn connection_tool() -> ToolDefinition {
    ToolDefinition {
        name: "confirm_connection",
        description: "确认模型能够按 JSON Schema 调用工具",
        input_schema: json!({
            "type": "object",
            "properties": { "ok": { "type": "boolean" } },
            "required": ["ok"],
            "additionalProperties": false
        }),
    }
}

/// 输出单工具请求的模型、轮次和工具缺失重试次数。
pub(super) fn log_request_attempt(
    config: &ProviderConfig,
    request: &ToolRequest<'_>,
    retry: usize,
) {
    debug_stage(
        request.trace_id,
        format!(
            "request.start: model={} turn={} missing_tool_retry={}/{}",
            config.model, request.turn, retry, MAX_MISSING_TOOL_RETRIES
        ),
    );
}

/// 输出多工具请求的模型、轮次和工具缺失重试次数。
pub(super) fn log_multi_request_attempt(
    config: &ProviderConfig,
    request: &MultiToolRequest<'_>,
    retry: usize,
) {
    debug_stage(
        request.trace_id,
        format!(
            "request.start: model={} turn={} missing_tool_retry={}/{}",
            config.model, request.turn, retry, MAX_MISSING_TOOL_RETRIES
        ),
    );
}

/// 判断无工具响应是否仍有重试额度，并输出本次重试状态。
pub(super) fn retry_missing_tool(retry: usize, error: &CommandError, trace_id: &str) -> bool {
    if error.code != "PROVIDER_TOOL_NOT_CALLED" {
        return false;
    }
    if retry >= MAX_MISSING_TOOL_RETRIES {
        debug_stage(
            trace_id,
            format!(
                "tool.missing: 已用完 {} 次额外重试",
                MAX_MISSING_TOOL_RETRIES
            ),
        );
        return false;
    }
    debug_stage(
        trace_id,
        format!(
            "tool.missing: 准备额外重试 {}/{}",
            retry + 1,
            MAX_MISSING_TOOL_RETRIES
        ),
    );
    true
}

/// 在 Codex 返回过载错误时等待指数退避时间，并决定是否继续重试。
pub(super) async fn retry_codex_overload(attempt: usize, error: &CommandError) -> bool {
    if error.code != "PROVIDER_OVERLOADED" || attempt + 1 >= CODEX_MAX_ATTEMPTS {
        return false;
    }
    let delay_ms = 500_u64 * 2_u64.pow(attempt as u32) + rand::random::<u64>() % 250;
    eprintln!(
        "[QuailCard] ChatGPT Codex 服务过载，{} ms 后进行第 {} 次重试",
        delay_ms,
        attempt + 2
    );
    tokio::time::sleep(Duration::from_millis(delay_ms)).await;
    true
}

/// 校验连接测试工具返回的确认值。
pub(super) fn validate_connection_arguments(arguments: Value) -> Result<(), CommandError> {
    if arguments.get("ok").and_then(Value::as_bool) == Some(true) {
        Ok(())
    } else {
        Err(CommandError::provider(
            "PROVIDER_TOOL_RESPONSE_INVALID",
            "模型工具调用没有返回预期确认值",
        ))
    }
}

/// 规范化并校验用户配置的 API 根地址。
pub(crate) fn normalize_base_url(value: &str) -> Result<String, CommandError> {
    let mut url =
        Url::parse(value.trim()).map_err(|_| CommandError::validation("BaseURL 不是有效地址"))?;
    if !url.username().is_empty()
        || url.password().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
    {
        return Err(CommandError::validation(
            "BaseURL 不能包含账号、密码、查询参数或片段",
        ));
    }
    let host = url
        .host_str()
        .ok_or_else(|| CommandError::validation("BaseURL 缺少主机名"))?;
    let local_http = url.scheme() == "http"
        && matches!(
            host.to_ascii_lowercase().as_str(),
            "localhost" | "127.0.0.1" | "::1"
        );
    if url.scheme() != "https" && !local_http {
        return Err(CommandError::validation(
            "BaseURL 必须使用 HTTPS，本机服务可以使用 HTTP",
        ));
    }
    if !url.path().ends_with('/') {
        let path = format!("{}/", url.path());
        url.set_path(&path);
    }
    Ok(url.to_string())
}

/// 计算最终聊天端点，所有路径均基于已规范化 URL 拼接。
pub(super) fn completion_endpoint(
    base_url: &str,
    protocol: ProviderProtocol,
) -> Result<Url, CommandError> {
    let normalized = normalize_base_url(base_url)?;
    let base =
        Url::parse(&normalized).map_err(|_| CommandError::validation("供应商 BaseURL 无效"))?;
    let path = match protocol {
        ProviderProtocol::OpenAiCompatible if base.path() == "/" => "v1/chat/completions",
        ProviderProtocol::OpenAiCompatible => "chat/completions",
        ProviderProtocol::AnthropicMessages
            if base.path().trim_end_matches('/').ends_with("/v1") =>
        {
            "messages"
        }
        ProviderProtocol::AnthropicMessages => "v1/messages",
    };
    base.join(path)
        .map_err(|_| CommandError::validation("无法拼接供应商请求地址"))
}
