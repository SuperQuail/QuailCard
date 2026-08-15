use std::time::Duration;

use reqwest::{redirect, Client, Request};
use serde_json::Value;
use uuid::Uuid;

use super::{
    request::{
        anthropic_body, anthropic_multi_body, openai_body, openai_multi_body,
        openai_responses_body, openai_responses_multi_body,
    },
    response::map_body_error,
    response::map_request_error,
    response::parse_anthropic_tool_calls,
    response::parse_openai_responses_batch,
    response::parse_openai_responses_tool_response,
    response::parse_openai_tool_calls,
    response::parse_tool_response,
    stream::{read_model_body, StreamProtocol},
    MultiToolRequest, ProviderProtocol, ToolCallBatch, ToolRequest,
};
use crate::{
    error::CommandError,
    models::{ProviderConfig, OPENAI_SUBSCRIPTION_ENDPOINT, OPENAI_SUBSCRIPTION_PROVIDER_TYPE},
};

#[path = "client_helpers.rs"]
mod client_helpers;

use client_helpers::{
    completion_endpoint, connection_tool, log_multi_request_attempt, log_request_attempt,
    retry_codex_overload, retry_missing_tool, validate_connection_arguments, CODEX_MAX_ATTEMPTS,
    MAX_MISSING_TOOL_RETRIES,
};

pub(crate) use client_helpers::normalize_base_url;

/// 复用连接池并统一执行两类供应商请求。
pub struct AiClient {
    http: Client,
}

impl AiClient {
    /// 创建禁用自动重定向的模型 HTTP 客户端。
    pub fn new() -> Result<Self, CommandError> {
        let http = Client::builder()
            .connect_timeout(Duration::from_secs(15))
            .timeout(Duration::from_secs(90))
            .redirect(redirect::Policy::none())
            .build()
            .map_err(|_| CommandError::provider("HTTP_CLIENT_ERROR", "无法初始化网络客户端"))?;
        Ok(Self { http })
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
        let expected_tool = tool_request.tool.name;
        for retry in 0..=MAX_MISSING_TOOL_RETRIES {
            log_request_attempt(config, &tool_request, retry);
            let request = self.build_tool_request(config, api_key, &tool_request, retry > 0)?;
            let response = self
                .http
                .execute(request)
                .await
                .map_err(map_request_error)?;
            let result =
                parse_tool_response(config, response, expected_tool, tool_request.trace_id).await;
            match result {
                Err(error) if retry_missing_tool(retry, &error, tool_request.trace_id) => continue,
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
        let expected_tool = tool_request.tool.name;
        for retry in 0..=MAX_MISSING_TOOL_RETRIES {
            log_request_attempt(config, &tool_request, retry);
            let result = self
                .call_openai_oauth_tool_once(
                    config,
                    access_token,
                    account_id,
                    &tool_request,
                    expected_tool,
                    retry > 0,
                )
                .await;
            match result {
                Err(error) if retry_missing_tool(retry, &error, tool_request.trace_id) => continue,
                result => return result,
            }
        }
        unreachable!("工具缺失重试循环必须在限定次数内返回")
    }

    /// 多工具请求：允许模型在可选工具间自主选择并返回全部调用。
    pub async fn call_multi_tool(
        &self,
        config: &ProviderConfig,
        api_key: &str,
        request: MultiToolRequest<'_>,
    ) -> Result<ToolCallBatch, CommandError> {
        let protocol = ProviderProtocol::parse(&config.protocol)?;
        let endpoint = completion_endpoint(&config.base_url, protocol)?;
        for retry in 0..=MAX_MISSING_TOOL_RETRIES {
            log_multi_request_attempt(config, &request, retry);
            let builder = match protocol {
                ProviderProtocol::OpenAiCompatible => self
                    .http
                    .post(endpoint.clone())
                    .bearer_auth(api_key)
                    .header("Accept", "text/event-stream")
                    .json(&openai_multi_body(config, &request, retry > 0)),
                ProviderProtocol::AnthropicMessages => self
                    .http
                    .post(endpoint.clone())
                    .header("x-api-key", api_key)
                    .header("anthropic-version", "2023-06-01")
                    .header("Accept", "text/event-stream")
                    .json(&anthropic_multi_body(config, &request, retry > 0)),
            };
            let response = builder
                .timeout(request.timeout)
                .send()
                .await
                .map_err(map_request_error)?;
            let status = response.status();
            let result = if status.is_success() {
                let stream_protocol = match protocol {
                    ProviderProtocol::OpenAiCompatible => StreamProtocol::OpenAiChat,
                    ProviderProtocol::AnthropicMessages => StreamProtocol::Anthropic,
                };
                let body = read_model_body(response, stream_protocol, request.trace_id).await?;
                match protocol {
                    ProviderProtocol::OpenAiCompatible => parse_openai_tool_calls(&body),
                    ProviderProtocol::AnthropicMessages => parse_anthropic_tool_calls(&body),
                }
                .map(|calls| ToolCallBatch {
                    calls,
                    continuation_items: Vec::new(),
                })
            } else {
                Err(map_body_error(status.as_u16(), response).await)
            };
            match result {
                Err(error) if retry_missing_tool(retry, &error, request.trace_id) => continue,
                result => return result,
            }
        }
        unreachable!("工具缺失重试循环必须在限定次数内返回")
    }

    /// 通过 ChatGPT Codex Responses 支持多工具自主选择，过载时自动重试。
    pub async fn call_openai_oauth_multi_tool(
        &self,
        config: &ProviderConfig,
        access_token: &str,
        account_id: Option<&str>,
        request: MultiToolRequest<'_>,
    ) -> Result<ToolCallBatch, CommandError> {
        if access_token.trim().is_empty() {
            return Err(CommandError::new(
                "PROVIDER_CREDENTIAL_MISSING",
                "OpenAI OAuth access token 为空",
            ));
        }
        for retry in 0..=MAX_MISSING_TOOL_RETRIES {
            log_multi_request_attempt(config, &request, retry);
            let result = self
                .call_openai_oauth_multi_tool_once(
                    config,
                    access_token,
                    account_id,
                    &request,
                    retry > 0,
                )
                .await;
            match result {
                Err(error) if retry_missing_tool(retry, &error, request.trace_id) => continue,
                result => return result,
            }
        }
        unreachable!("工具缺失重试循环必须在限定次数内返回")
    }

    /// 执行一次 Codex 单工具逻辑请求，服务过载时在内部指数退避。
    async fn call_openai_oauth_tool_once(
        &self,
        config: &ProviderConfig,
        access_token: &str,
        account_id: Option<&str>,
        tool_request: &ToolRequest<'_>,
        expected_tool: &str,
        retry_missing_tool: bool,
    ) -> Result<Value, CommandError> {
        for attempt in 0..CODEX_MAX_ATTEMPTS {
            let request = self.build_openai_oauth_request(
                config,
                access_token,
                account_id,
                tool_request,
                retry_missing_tool,
            )?;
            let response = self
                .http
                .execute(request)
                .await
                .map_err(map_request_error)?;
            let result = parse_openai_responses_tool_response(
                response,
                expected_tool,
                tool_request.trace_id,
            )
            .await;
            match result {
                Err(error) if retry_codex_overload(attempt, &error).await => continue,
                result => return result,
            }
        }
        unreachable!("Codex 过载重试循环必须在限定次数内返回")
    }

    /// 执行一次 Codex 多工具逻辑请求，服务过载时在内部指数退避。
    async fn call_openai_oauth_multi_tool_once(
        &self,
        config: &ProviderConfig,
        access_token: &str,
        account_id: Option<&str>,
        request: &MultiToolRequest<'_>,
        retry_missing_tool: bool,
    ) -> Result<ToolCallBatch, CommandError> {
        for attempt in 0..CODEX_MAX_ATTEMPTS {
            let mut builder = self
                .http
                .post(OPENAI_SUBSCRIPTION_ENDPOINT)
                .bearer_auth(access_token)
                .header("Accept", "text/event-stream")
                .header("originator", "quailcard")
                .header("User-Agent", "QuailCard/0.1.0")
                .json(&openai_responses_multi_body(
                    config,
                    request,
                    retry_missing_tool,
                ));
            if let Some(account_id) = account_id.filter(|value| !value.trim().is_empty()) {
                builder = builder.header("ChatGPT-Account-Id", account_id);
            }
            let response = builder
                .timeout(request.timeout)
                .send()
                .await
                .map_err(map_request_error)?;
            let status = response.status();
            let result = if status.is_success() {
                let body =
                    read_model_body(response, StreamProtocol::OpenAiResponses, request.trace_id)
                        .await?;
                parse_openai_responses_batch(&body)
            } else {
                Err(map_body_error(status.as_u16(), response).await)
            };
            match result {
                Err(error) if retry_codex_overload(attempt, &error).await => continue,
                result => return result,
            }
        }
        unreachable!("Codex 过载重试循环必须在限定次数内返回")
    }

    /// 根据供应商协议构造单工具调用请求。
    fn build_tool_request(
        &self,
        config: &ProviderConfig,
        api_key: &str,
        tool_request: &ToolRequest<'_>,
        retry_missing_tool: bool,
    ) -> Result<Request, CommandError> {
        if api_key.trim().is_empty() {
            return Err(CommandError::validation("API Key 不能为空"));
        }
        let protocol = ProviderProtocol::parse(&config.protocol)?;
        let endpoint = completion_endpoint(&config.base_url, protocol)?;
        let builder = match protocol {
            ProviderProtocol::OpenAiCompatible => self
                .http
                .post(endpoint)
                .bearer_auth(api_key)
                .header("Accept", "text/event-stream")
                .json(&openai_body(config, tool_request, retry_missing_tool)),
            ProviderProtocol::AnthropicMessages => self
                .http
                .post(endpoint)
                .header("x-api-key", api_key)
                .header("anthropic-version", "2023-06-01")
                .header("Accept", "text/event-stream")
                .json(&anthropic_body(config, tool_request, retry_missing_tool)),
        };
        builder
            .timeout(tool_request.timeout)
            .build()
            .map_err(|_| CommandError::provider("PROVIDER_REQUEST_INVALID", "无法构造模型请求"))
    }

    /// 构造不会使用供应商 BaseURL 的 ChatGPT OAuth Responses 请求。
    fn build_openai_oauth_request(
        &self,
        config: &ProviderConfig,
        access_token: &str,
        account_id: Option<&str>,
        tool_request: &ToolRequest<'_>,
        retry_missing_tool: bool,
    ) -> Result<Request, CommandError> {
        if access_token.trim().is_empty() {
            return Err(CommandError::new(
                "PROVIDER_CREDENTIAL_MISSING",
                "OpenAI OAuth access token 为空",
            ));
        }
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
        let mut builder = self
            .http
            .post(OPENAI_SUBSCRIPTION_ENDPOINT)
            .bearer_auth(access_token)
            .header("Accept", "text/event-stream")
            .header("originator", "quailcard")
            .header("User-Agent", "QuailCard/0.1.0")
            .json(&openai_responses_body(
                config,
                tool_request,
                retry_missing_tool,
            ));
        if let Some(account_id) = account_id.filter(|value| !value.trim().is_empty()) {
            builder = builder.header("ChatGPT-Account-Id", account_id);
        }
        builder
            .timeout(tool_request.timeout)
            .build()
            .map_err(|_| CommandError::provider("PROVIDER_REQUEST_INVALID", "无法构造模型请求"))
    }
}

#[cfg(test)]
#[path = "client_tests.rs"]
mod tests;
