//! 单次笔记请求的安全诊断；不保留提示词、正文或模型返回的原始字段。
use crate::{error::CommandError, services::agent_ports::AgentModel};
use serde_json::json;
use std::{
    sync::atomic::{AtomicUsize, Ordering},
    time::Instant,
};

type Log<'a> = &'a (dyn Fn(&str) + Send + Sync);

struct RequestLog<'a> {
    trace: String,
    phase: &'a str,
    log: Log<'a>,
    started: Instant,
    finished: bool,
}

impl RequestLog<'_> {
    /// 同一请求 ID 同时出现在任务面板和后端控制台，正文不进入日志。
    fn emit(&self, event: &str) {
        let line = format!(
            "模型请求 trace={} phase={} elapsed_ms={} {}",
            self.trace,
            self.phase,
            self.started.elapsed().as_millis(),
            event
        );
        eprintln!("[QuailCard][VideoNote] {line}");
        (self.log)(&line);
    }
}

impl Drop for RequestLog<'_> {
    /// Future 被取消或中途丢弃时补上中断记录，不伪装成超时或正常完成。
    fn drop(&mut self) {
        if !self.finished {
            self.emit("outcome=interrupted");
        }
    }
}

/// 工具集为空的纯文本请求；日志仅包含阶段、计数、耗时和固定错误码。
pub(super) async fn call(
    model: &dyn AgentModel,
    prompt: &str,
    phase: &str,
    log: Log<'_>,
) -> Result<String, CommandError> {
    let mut diagnostic = RequestLog {
        trace: uuid::Uuid::now_v7().to_string(),
        phase,
        log,
        started: Instant::now(),
        finished: false,
    };
    let system = "你是视频笔记编辑。忠实理解语境与作者意图，允许结合上下文保守修正明显的口误、笔误和语音转写错误。只输出 Markdown 正文，不要输出解释或代码块围栏。";
    diagnostic.emit(&format!(
        "outcome=started prompt_chars={} system_chars={}",
        prompt.chars().count(),
        system.chars().count()
    ));
    let messages = [json!({"role": "user", "content": prompt})];
    let streamed = AtomicUsize::new(0);
    let delta = |text: &str| {
        streamed.fetch_add(text.chars().count(), Ordering::Relaxed);
    };
    // 视频笔记只要正文，推理内容不参与进度统计。
    let reasoning = |_: &str| {};
    let result = model
        .call_traced(
            system,
            &messages,
            &[],
            &delta,
            &reasoning,
            &diagnostic.trace,
        )
        .await;
    match result {
        Err(error) => {
            diagnostic.emit(&format!(
                "outcome=failed code={} streamed_chars={}",
                safe_code(error.code),
                streamed.load(Ordering::Relaxed)
            ));
            diagnostic.finished = true;
            Err(error)
        }
        Ok(reply) => {
            let text = reply.text.trim().to_string();
            diagnostic.emit(&format!(
                "outcome={} reply_chars={} streamed_chars={} tool_calls={}",
                if text.is_empty() {
                    "empty"
                } else {
                    "completed"
                },
                text.chars().count(),
                streamed.load(Ordering::Relaxed),
                reply.calls.len()
            ));
            diagnostic.finished = true;
            if text.is_empty() {
                Err(CommandError::new(
                    "VIDEO_NOTE_FAILED",
                    "模型没有返回笔记内容，请重试",
                ))
            } else {
                Ok(text)
            }
        }
    }
}

/// 扩展实现即使返回自定义错误码，也不能把任意字符串透传进诊断。
fn safe_code(code: &str) -> &'static str {
    match code {
        "PROVIDER_TIMEOUT" => "PROVIDER_TIMEOUT",
        "PROVIDER_UNREACHABLE" => "PROVIDER_UNREACHABLE",
        "PROVIDER_REQUEST_FAILED" => "PROVIDER_REQUEST_FAILED",
        "PROVIDER_AUTH_FAILED" => "PROVIDER_AUTH_FAILED",
        "PROVIDER_RATE_LIMITED" => "PROVIDER_RATE_LIMITED",
        "PROVIDER_OVERLOADED" => "PROVIDER_OVERLOADED",
        "PROVIDER_SERVER_ERROR" => "PROVIDER_SERVER_ERROR",
        "PROVIDER_RESPONSE_INVALID" => "PROVIDER_RESPONSE_INVALID",
        "PROVIDER_RESPONSE_TOO_LARGE" => "PROVIDER_RESPONSE_TOO_LARGE",
        "PROVIDER_STREAM_INTERRUPTED" => "PROVIDER_STREAM_INTERRUPTED",
        "VIDEO_CANCELLED" => "VIDEO_CANCELLED",
        _ => "OTHER_ERROR",
    }
}

#[cfg(test)]
#[path = "video_note_call_tests.rs"]
mod tests;
