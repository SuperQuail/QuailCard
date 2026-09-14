//! 模型调用运行时：解析路由、发送、折叠 chunk、归一失败。

use std::time::Duration;

use reqwest::{Client, Response};
use serde_json::Value;

use super::adapter::{route_key, AdapterRegistry, LlmAdapter};
use super::assembler::BlockAssembler;
use super::chunk::StreamChunk;
use super::diagnostics::{RequestDiagnostics, CONNECT_TIMEOUT_SECS};
use super::echo;
use super::request::{AssistantTurn, Credential, ModelRequest, ModelRoute};
use super::sse::{is_event_stream, SseParser};
use super::vocabulary::{ContentBlock, FinishReason};
use crate::ai::{ProviderProtocol, ToolArguments, ToolCallResult};
use crate::error::CommandError;

#[path = "runtime_errors.rs"]
mod errors;
use errors::{
    command_error, idle_timeout, incomplete, invalid_response, map_body_error, map_request_error,
    map_stream_error,
};

#[cfg(test)]
#[path = "runtime_tests.rs"]
mod tests;

const MAX_RESPONSE_BYTES: usize = 5 * 1024 * 1024;

/// 调用方关心的流式增量；思考与可见文字分开，互不污染。
#[derive(Clone, Copy, Default)]
pub(crate) struct StreamSinks<'a> {
    pub text: Option<&'a (dyn Fn(&str) + Send + Sync)>,
    pub reasoning: Option<&'a (dyn Fn(&str) + Send + Sync)>,
}

/// 供应商调用运行时；协议细节全部在 adapter 内。
pub(crate) struct LlmRuntime {
    http: Client,
    adapters: AdapterRegistry,
}

impl LlmRuntime {
    /// 创建禁用自动重定向的连接池。
    pub(crate) fn new() -> Result<Self, CommandError> {
        let http = Client::builder()
            .connect_timeout(Duration::from_secs(CONNECT_TIMEOUT_SECS))
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .map_err(|_| CommandError::provider("HTTP_CLIENT_ERROR", "无法初始化网络客户端"))?;
        Ok(Self {
            http,
            adapters: AdapterRegistry::default(),
        })
    }

    /// 注册表只读访问，供组合根登记 adapter。
    pub(crate) fn adapters(&self) -> &AdapterRegistry {
        &self.adapters
    }

    /// 执行一次模型调用；失败已归一为安全错误，工具参数保留原始诊断。
    pub(crate) async fn stream(
        &self,
        route: &ModelRoute,
        request: &ModelRequest,
        credential: &Credential,
        trace: &str,
        tag: &'static str,
        sinks: StreamSinks<'_>,
    ) -> Result<AssistantTurn, CommandError> {
        let key = route_key(route.auth_type, protocol_name(route.protocol));
        let adapter = self.adapters.resolve(&key)?;
        let wire = adapter.build(&self.http, route, request, credential)?;
        // 传输层只有空闲看门狗：请求头或流数据在 idle 内没有任何新数据才算超时，
        // 持续输出的思考模型不会被整请求总时限打断。
        let idle = Duration::from_millis(route.idle_timeout_ms);
        let response = tokio::time::timeout(idle, self.http.execute(wire))
            .await
            .map_err(|_| idle_timeout())?
            .map_err(map_request_error)?;
        let status = response.status();
        let streaming = is_event_stream(&response);
        let mut diagnostics = RequestDiagnostics::new(trace, tag, adapter.protocol());
        diagnostics.status = Some(status.as_u16());
        diagnostics.streaming = streaming;
        diagnostics.idle_timeout_ms = route.idle_timeout_ms;
        diagnostics.begin();
        let result = read_and_assemble(
            adapter.as_ref(),
            response,
            streaming,
            idle,
            &mut diagnostics,
            sinks,
        )
        .await;
        echo::finish();
        if let Ok(turn) = &result {
            for call in &turn.calls {
                echo::tool_call(&call.name, &call.arguments.replay_value());
            }
        }
        match &result {
            Ok(turn) => {
                diagnostics.record_calls(turn.calls.iter().map(|call| call.name.as_str()));
                let text_bytes = turn.text.len();
                diagnostics.finish_success(text_bytes);
            }
            Err(error) => diagnostics.finish_error(error),
        }
        result
    }
}

/// 协议枚举到注册表标签；只用于路由，不进入日志。
fn protocol_name(protocol: ProviderProtocol) -> &'static str {
    match protocol {
        ProviderProtocol::OpenAiCompatible => "OpenAI Compatible",
        ProviderProtocol::AnthropicMessages => "Anthropic Messages",
    }
}

/// 读取响应并按 chunk 折叠；SSE 与普通 JSON 走同一套结果类型。
async fn read_and_assemble(
    adapter: &dyn LlmAdapter,
    mut response: Response,
    streaming: bool,
    idle: Duration,
    diagnostics: &mut RequestDiagnostics,
    sinks: StreamSinks<'_>,
) -> Result<AssistantTurn, CommandError> {
    if !response.status().is_success() {
        let status = response.status().as_u16();
        return Err(map_body_error(status, response, idle).await);
    }
    let mut assembler = BlockAssembler::default();
    let mut parser = SseParser::default();
    let mut body = Vec::new();
    let mut done = false;
    loop {
        let chunk = match tokio::time::timeout(idle, response.chunk()).await {
            Ok(Ok(Some(chunk))) => chunk,
            Ok(Ok(None)) => break,
            Ok(Err(error)) => return Err(map_stream_error(error)),
            Err(_) => return Err(idle_timeout()),
        };
        diagnostics.observe_chunk(&chunk);
        if body.len() + chunk.len() > MAX_RESPONSE_BYTES {
            return Err(CommandError::provider(
                "PROVIDER_RESPONSE_TOO_LARGE",
                "模型响应超过 5 MiB 限制",
            ));
        }
        body.extend_from_slice(&chunk);
        if streaming {
            done |= absorb_events(
                adapter,
                &mut parser,
                &chunk,
                &mut assembler,
                diagnostics,
                sinks,
            )?;
        }
    }
    if streaming {
        done |= absorb_tail(adapter, &mut parser, &mut assembler, diagnostics, sinks)?;
    } else {
        absorb_json(adapter, &body, &mut assembler, diagnostics, sinks)?;
        done = true;
    }
    diagnostics.checkpoint("body_complete");
    finalize(assembler, streaming, done)
}

/// 处理到达的一段字节中的完整事件。
fn absorb_events(
    adapter: &dyn LlmAdapter,
    parser: &mut SseParser,
    bytes: &[u8],
    assembler: &mut BlockAssembler,
    diagnostics: &mut RequestDiagnostics,
    sinks: StreamSinks<'_>,
) -> Result<bool, CommandError> {
    let mut done = false;
    for data in parser.push(bytes)? {
        done |= absorb_data(adapter, &data, assembler, diagnostics, sinks)?;
    }
    Ok(done)
}

/// 处理流结束时残留的尾事件。
fn absorb_tail(
    adapter: &dyn LlmAdapter,
    parser: &mut SseParser,
    assembler: &mut BlockAssembler,
    diagnostics: &mut RequestDiagnostics,
    sinks: StreamSinks<'_>,
) -> Result<bool, CommandError> {
    match parser.finish()? {
        Some(data) => absorb_data(adapter, &data, assembler, diagnostics, sinks),
        None => Ok(false),
    }
}

/// 处理单个 data 负载；返回是否见到终止标记。
fn absorb_data(
    adapter: &dyn LlmAdapter,
    data: &str,
    assembler: &mut BlockAssembler,
    diagnostics: &mut RequestDiagnostics,
    sinks: StreamSinks<'_>,
) -> Result<bool, CommandError> {
    if data == "[DONE]" {
        return Ok(true);
    }
    match serde_json::from_str::<Value>(data) {
        Ok(event) => {
            diagnostics.observe_event(&event);
            for piece in adapter.translate(&event) {
                echo::push(&piece);
                forward_stream(&piece, sinks);
                assembler.push(piece);
            }
            return Ok(adapter.is_terminal(&event));
        }
        Err(_) => diagnostics.invalid_events += 1,
    }
    Ok(false)
}

/// 非流式响应整体翻译成 chunk。
fn absorb_json(
    adapter: &dyn LlmAdapter,
    body: &[u8],
    assembler: &mut BlockAssembler,
    diagnostics: &mut RequestDiagnostics,
    sinks: StreamSinks<'_>,
) -> Result<(), CommandError> {
    let parsed = serde_json::from_slice::<Value>(body).map_err(|_| invalid_response())?;
    diagnostics.observe_event(&parsed);
    for piece in adapter.translate_json(&parsed) {
        echo::push(&piece);
        forward_stream(&piece, sinks);
        assembler.push(piece);
    }
    Ok(())
}

/// 按增量类型分发：可见文本与思考各走自己的回调，工具参数不外发。
fn forward_stream(piece: &StreamChunk, sinks: StreamSinks<'_>) {
    match piece {
        StreamChunk::TextDelta { text, .. } => {
            if let Some(callback) = sinks.text {
                callback(text);
            }
        }
        StreamChunk::ReasoningDelta { text, .. } => {
            if let Some(callback) = sinks.reasoning {
                callback(text);
            }
        }
        _ => {}
    }
}

/// 收敛折叠结果：补齐终止原因、归一失败、解码工具调用。
fn finalize(
    mut assembler: BlockAssembler,
    streaming: bool,
    done: bool,
) -> Result<AssistantTurn, CommandError> {
    if assembler.finish().is_none() {
        if !streaming {
            return Err(invalid_response());
        }
        if !done {
            return Err(incomplete());
        }
        let reason = if assembler.tool_calls().is_empty() {
            FinishReason::Stop
        } else {
            FinishReason::ToolCalls
        };
        assembler.push(StreamChunk::Finish {
            reason,
            failure: None,
            replay: None,
        });
    }
    let (reason, failure) = assembler.finish().expect("终止原因已补齐");
    if let Some(failure) = failure {
        return Err(command_error(failure));
    }
    if matches!(reason, FinishReason::Error | FinishReason::Aborted) {
        return Err(CommandError::provider(
            "PROVIDER_REQUEST_FAILED",
            "供应商流以失败结束，请稍后重试",
        ));
    }
    let blocks = assembler.blocks();
    let text = blocks
        .iter()
        .filter_map(|block| match block {
            ContentBlock::Text { text } => Some(text.as_str()),
            _ => None,
        })
        .collect();
    let calls = assembler
        .tool_calls()
        .into_iter()
        .enumerate()
        .map(|(index, call)| ToolCallResult {
            id: if call.id.is_empty() {
                format!("call_{index}")
            } else {
                call.id
            },
            item_id: call.item_id,
            name: call.name,
            arguments: ToolArguments::parse(&call.arguments),
        })
        .collect();
    Ok(AssistantTurn {
        text,
        blocks,
        calls,
        usage: assembler.usage().cloned(),
        finish: reason,
        failure: None,
        replay: assembler.replay().cloned(),
    })
}
