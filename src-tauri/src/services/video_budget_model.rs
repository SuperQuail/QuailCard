//! 视频模型逻辑请求的唯一传输重试归属；不改普通聊天/gateway 策略。
use super::{cancelled, VideoBudget};
use crate::{
    ai::ToolDefinition,
    services::{
        agent_ports::{AgentFuture, AgentModel, AgentModelReply},
        video_tasks::VideoControl,
    },
};
use rand::Rng;
use serde_json::Value;
use std::time::Duration;

/// 四字段保持组合根可直接包装现有 ConfiguredAgentModel；任务身份从控制快照取得。
pub(crate) struct BudgetedModel<'a> {
    pub inner: &'a dyn AgentModel,
    pub budget: &'a VideoBudget,
    pub provider_id: &'a str,
    pub control: &'a VideoControl,
}

impl AgentModel for BudgetedModel<'_> {
    /// 无追踪入口仍消耗同一逻辑请求预算，不通过另一层重试包装。
    fn call<'a>(
        &'a self,
        system: &'a str,
        messages: &'a [Value],
        tools: &'a [ToolDefinition],
        delta: &'a (dyn Fn(&str) + Send + Sync),
        reasoning: &'a (dyn Fn(&str) + Send + Sync),
    ) -> AgentFuture<'a, AgentModelReply> {
        self.call_traced(system, messages, tools, delta, reasoning, "")
    }

    /// 每个逻辑请求至多三次实际调用；许可覆盖流结束，退避前归还。
    fn call_traced<'a>(
        &'a self,
        system: &'a str,
        messages: &'a [Value],
        tools: &'a [ToolDefinition],
        delta: &'a (dyn Fn(&str) + Send + Sync),
        reasoning: &'a (dyn Fn(&str) + Send + Sync),
        trace: &'a str,
    ) -> AgentFuture<'a, AgentModelReply> {
        Box::pin(async move {
            let task = self.control.snapshot().task_id;
            for attempt in 1..=3 {
                let permit = self
                    .budget
                    .model(self.provider_id, &task, self.control)
                    .await?;
                eprintln!(
                    "VIDEO_MODEL task_id={} provider_id={} attempt={}",
                    task, self.provider_id, attempt
                );
                let result = tokio::select! {
                    biased;
                    _ = self.control.cancelled() => return Err(cancelled()),
                    result = self.inner.call_traced(system, messages, tools, delta, reasoning, trace) => result,
                };
                let error = match result {
                    Ok(reply) => return Ok(reply),
                    Err(error) => error,
                };
                if !retryable(error.code) {
                    return Err(error);
                }
                let delay = retry_delay(attempt, error.retry_after_ms);
                // 最后一次 429 也发布冷却，不能让其它任务立即重击同供应商。
                // HTTP 429 也可能由既有正文分类投影成 OVERLOADED，因此过载同样共享冷却。
                if matches!(error.code, "PROVIDER_RATE_LIMITED" | "PROVIDER_OVERLOADED")
                    || error.retry_after_ms.is_some()
                {
                    self.budget.cool_down(self.provider_id, delay);
                }
                drop(permit);
                if attempt == 3 {
                    return Err(error);
                }
                tokio::select! {
                    biased;
                    _ = self.control.cancelled() => return Err(cancelled()),
                    _ = tokio::time::sleep(delay) => {},
                }
            }
            unreachable!("三次传输尝试必定返回")
        })
    }
}

/// 仅允许既有可恢复传输分类；认证、参数与视觉解析失败不盲重试。
fn retryable(code: &str) -> bool {
    matches!(
        code,
        "PROVIDER_TIMEOUT"
            | "PROVIDER_UNREACHABLE"
            | "PROVIDER_REQUEST_FAILED"
            | "PROVIDER_OVERLOADED"
            | "PROVIDER_SERVER_ERROR"
            | "PROVIDER_RATE_LIMITED"
    )
}

/// 服务端等待是下限；缺失提示采用带抖动指数退避，随机源不跨 await。
fn retry_delay(attempt: u32, retry_after_ms: Option<u64>) -> Duration {
    let jitter = rand::thread_rng().gen_range(0..=100);
    Duration::from_millis(
        retry_after_ms
            .unwrap_or(250 * (1 << (attempt - 1)))
            .saturating_add(jitter),
    )
}
