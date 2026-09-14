use std::time::Duration;

use serde_json::Value;
use uuid::Uuid;

use super::{
    bridge,
    llm::request::{AssistantTurn, Credential},
    llm::runtime::{LlmRuntime, StreamSinks},
    MultiToolRequest, ProviderProtocol, ToolCallBatch, ToolCallResult, ToolRequest,
};
use crate::{
    error::CommandError,
    models::{ProviderConfig, OPENAI_SUBSCRIPTION_PROVIDER_TYPE},
};

#[path = "client_helpers.rs"]
mod client_helpers;

use client_helpers::{
    connection_tool, retry_codex_overload, retry_missing_tool, validate_connection_arguments,
    CODEX_MAX_ATTEMPTS, MAX_MISSING_TOOL_RETRIES,
};

pub(crate) use client_helpers::{completion_endpoint, normalize_base_url};

/// 非 Agent 路径的模型入口（判定、工具调用、连接测试），统一经 LlmRuntime。
pub(crate) struct ProviderGateway {
    runtime: LlmRuntime,
}

impl ProviderGateway {
    /// 复用组合根，与 Agent 共用同一份已注册 adapter 的运行时。
    pub(crate) fn new() -> Result<Self, CommandError> {
        Ok(Self {
            runtime: super::configured_runtime()?,
        })
    }

    /// 调用最小工具以验证地址、密钥、模型和工具能力。
    pub async fn test_connection(
        &self,
        config: &ProviderConfig,
        api_key: &str,
    ) -> Result<(), CommandError> {
        let tool = connection_tool();
        let trace_id = Uuid::now_v7().to_string();
        let arguments = self
            .call_tool(
                config,
                api_key,
                ToolRequest {
                    trace_id: &trace_id,
                    turn: 1,
                    system_prompt: "你正在执行连接与工具能力测试，必须调用指定工具。",
                    user_prompt: "请调用 confirm_connection，并将 ok 设置为 true。",
                    images: &[],
                    tool: &tool,
                    max_tokens: 64,
                    timeout: Duration::from_secs(15),
                },
            )
            .await?;
        validate_connection_arguments(arguments)
    }

    /// 使用 ChatGPT OAuth 的 Responses 端点验证模型和工具能力。
    pub async fn test_openai_oauth_connection(
        &self,
        config: &ProviderConfig,
        access_token: &str,
        account_id: Option<&str>,
    ) -> Result<(), CommandError> {
        let tool = connection_tool();
        let trace_id = Uuid::now_v7().to_string();
        let arguments = self
            .call_openai_oauth_tool(
                config,
                access_token,
                account_id,
                ToolRequest {
                    trace_id: &trace_id,
                    turn: 1,
                    system_prompt: "你正在执行连接与工具能力测试，必须调用指定工具。",
                    user_prompt: "请调用 confirm_connection，并将 ok 设置为 true。",
                    images: &[],
                    tool: &tool,
                    max_tokens: 64,
                    timeout: Duration::from_secs(15),
                },
            )
            .await?;
        validate_connection_arguments(arguments)
    }

    /// 请求模型调用指定工具并返回经过 JSON 解析的参数。
    pub async fn call_tool(
        &self,
        config: &ProviderConfig,
        api_key: &str,
        tool_request: ToolRequest<'_>,
    ) -> Result<Value, CommandError> {
        if api_key.trim().is_empty() {
            return Err(CommandError::validation("API Key 不能为空"));
        }
        let expected_tool = tool_request.tool.name;
        let credential = Credential::ApiKey(zeroize::Zeroizing::new(api_key.to_string()));
        for retry in 0..=MAX_MISSING_TOOL_RETRIES {
            let route = bridge::route(
                config,
                "api_key",
                Some(tool_request.max_tokens),
                Some(0.2),
                false,
                tool_request.timeout.as_millis() as u64,
                tool_request.trace_id,
            )?;
            let model_request = bridge::single_request(&tool_request, retry > 0);
            let result = self
                .runtime
                .stream(
                    &route,
                    &model_request,
                    &credential,
                    tool_request.trace_id,
                    "CardGen",
                    StreamSinks::default(),
                )
                .await
                .and_then(|turn| expected_call(turn.calls, expected_tool));
            match result {
                Err(error) if retry_missing_tool(retry, &error) => continue,
                result => return result,
            }
        }
        unreachable!("工具缺失重试循环必须在限定次数内返回")
    }

    /// 通过 ChatGPT Codex Responses 调用单个函数工具，过载时自动重试。
    pub async fn call_openai_oauth_tool(
        &self,
        config: &ProviderConfig,
        access_token: &str,
        account_id: Option<&str>,
        tool_request: ToolRequest<'_>,
    ) -> Result<Value, CommandError> {
        validate_oauth(config)?;
        let credential = oauth_credential(access_token, account_id)?;
        let expected_tool = tool_request.tool.name;
        for retry in 0..=MAX_MISSING_TOOL_RETRIES {
            let result = self
                .openai_oauth_tool_once(config, &credential, &tool_request, expected_tool, retry)
                .await;
            match result {
                Err(error) if retry_missing_tool(retry, &error) => continue,
                result => return result,
            }
        }
        unreachable!("工具缺失重试循环必须在限定次数内返回")
    }

    /// 多工具请求：允许模型在可选工具间自主选择并返回全部调用。
    /// delta 只接收正文，不接收思考；None 表示不需要流式进度。
    pub async fn call_multi_tool_with_delta(
        &self,
        config: &ProviderConfig,
        api_key: &str,
        request: MultiToolRequest<'_>,
        delta: Option<&(dyn Fn(&str) + Send + Sync)>,
    ) -> Result<ToolCallBatch, CommandError> {
        if api_key.trim().is_empty() {
            return Err(CommandError::validation("API Key 不能为空"));
        }
        let credential = Credential::ApiKey(zeroize::Zeroizing::new(api_key.to_string()));
        for retry in 0..=MAX_MISSING_TOOL_RETRIES {
            let route = bridge::route(
                config,
                "api_key",
                Some(request.max_tokens),
                Some(0.2),
                true,
                request.timeout.as_millis() as u64,
                request.trace_id,
            )?;
            let model_request = bridge::multi_request(&request, retry > 0);
            let result = self
                .runtime
                .stream(
                    &route,
                    &model_request,
                    &credential,
                    request.trace_id,
                    "CardGen",
                    StreamSinks {
                        text: delta,
                        reasoning: None,
                    },
                )
                .await
                .and_then(batch_from_turn);
            match result {
                Err(error) if retry_missing_tool(retry, &error) => continue,
                result => return result,
            }
        }
        unreachable!("工具缺失重试循环必须在限定次数内返回")
    }

    /// 通过 ChatGPT Codex Responses 支持多工具自主选择，过载时自动重试。
    /// delta 只接收正文；None 表示不需要流式进度。
    pub async fn call_openai_oauth_multi_tool_with_delta(
        &self,
        config: &ProviderConfig,
        access_token: &str,
        account_id: Option<&str>,
        request: MultiToolRequest<'_>,
        delta: Option<&(dyn Fn(&str) + Send + Sync)>,
    ) -> Result<ToolCallBatch, CommandError> {
        validate_oauth(config)?;
        let credential = oauth_credential(access_token, account_id)?;
        for retry in 0..=MAX_MISSING_TOOL_RETRIES {
            let result = self
                .openai_oauth_multi_tool_once(config, &credential, &request, retry, delta)
                .await;
            match result {
                Err(error) if retry_missing_tool(retry, &error) => continue,
                result => return result,
            }
        }
        unreachable!("工具缺失重试循环必须在限定次数内返回")
    }

    /// 执行一次 Codex 单工具逻辑请求，服务过载时在内部指数退避。
    async fn openai_oauth_tool_once(
        &self,
        config: &ProviderConfig,
        credential: &Credential,
        tool_request: &ToolRequest<'_>,
        expected_tool: &str,
        retry: usize,
    ) -> Result<Value, CommandError> {
        for attempt in 0..CODEX_MAX_ATTEMPTS {
            let route = bridge::route(
                config,
                "openai_oauth",
                None,
                None,
                false,
                tool_request.timeout.as_millis() as u64,
                tool_request.trace_id,
            )?;
            let model_request = bridge::single_request(tool_request, retry > 0);
            let result = self
                .runtime
                .stream(
                    &route,
                    &model_request,
                    credential,
                    tool_request.trace_id,
                    "CardGen",
                    StreamSinks::default(),
                )
                .await
                .and_then(|turn| expected_call(turn.calls, expected_tool));
            match result {
                Err(error) if retry_codex_overload(attempt, &error).await => continue,
                result => return result,
            }
        }
        unreachable!("Codex 过载重试循环必须在限定次数内返回")
    }

    /// 执行一次 Codex 多工具逻辑请求，服务过载时在内部指数退避。
    async fn openai_oauth_multi_tool_once(
        &self,
        config: &ProviderConfig,
        credential: &Credential,
        request: &MultiToolRequest<'_>,
        retry: usize,
        delta: Option<&(dyn Fn(&str) + Send + Sync)>,
    ) -> Result<ToolCallBatch, CommandError> {
        for attempt in 0..CODEX_MAX_ATTEMPTS {
            let route = bridge::route(
                config,
                "openai_oauth",
                None,
                None,
                true,
                request.timeout.as_millis() as u64,
                request.trace_id,
            )?;
            let model_request = bridge::multi_request(request, retry > 0);
            let result = self
                .runtime
                .stream(
                    &route,
                    &model_request,
                    credential,
                    request.trace_id,
                    "CardGen",
                    StreamSinks {
                        text: delta,
                        reasoning: None,
                    },
                )
                .await
                .and_then(batch_from_turn);
            match result {
                Err(error) if retry_codex_overload(attempt, &error).await => continue,
                result => return result,
            }
        }
        unreachable!("Codex 过载重试循环必须在限定次数内返回")
    }
}

/// OAuth 只能用于 OpenAI 订阅供应商，错误在请求前显式返回。
fn validate_oauth(config: &ProviderConfig) -> Result<(), CommandError> {
    if ProviderProtocol::parse(&config.protocol)? != ProviderProtocol::OpenAiCompatible {
        return Err(CommandError::validation(
            "OpenAI OAuth 只能用于 OpenAI Compatible 协议",
        ));
    }
    if config.provider_type != OPENAI_SUBSCRIPTION_PROVIDER_TYPE {
        return Err(CommandError::new(
            "PROVIDER_AUTH_MISMATCH",
            "ChatGPT OAuth 只能用于 OpenAI 订阅供应商",
        ));
    }
    Ok(())
}

/// OAuth 凭据按请求构造，空 token 提前拒绝。
fn oauth_credential(
    access_token: &str,
    account_id: Option<&str>,
) -> Result<Credential, CommandError> {
    if access_token.trim().is_empty() {
        return Err(CommandError::new(
            "PROVIDER_CREDENTIAL_MISSING",
            "OpenAI OAuth access token 为空",
        ));
    }
    Ok(Credential::OAuth {
        access_token: zeroize::Zeroizing::new(access_token.to_string()),
        account_id: account_id.map(str::to_string),
    })
}

/// 无工具调用按可重试错误处理；否则只接受预期工具。
fn expected_call(calls: Vec<ToolCallResult>, expected_tool: &str) -> Result<Value, CommandError> {
    if calls.is_empty() {
        return Err(CommandError::provider(
            "PROVIDER_TOOL_NOT_CALLED",
            "模型响应结束但没有调用工具",
        ));
    }
    bridge::select_expected_call(calls, expected_tool)
}

/// 无工具调用按可重试错误处理；Responses 续传项从重放信封提取。
fn batch_from_turn(turn: AssistantTurn) -> Result<ToolCallBatch, CommandError> {
    if turn.calls.is_empty() {
        return Err(CommandError::provider(
            "PROVIDER_TOOL_NOT_CALLED",
            "模型响应结束但没有调用工具",
        ));
    }
    let continuation_items = bridge::continuation_items(turn.replay.as_ref());
    Ok(ToolCallBatch {
        calls: turn.calls,
        continuation_items,
    })
}

#[cfg(test)]
#[path = "client_tests.rs"]
mod tests;
