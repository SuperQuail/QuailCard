use crate::error::CommandError;
use std::sync::OnceLock;
use std::time::Instant;
use uuid::Uuid;

pub(crate) const CONNECT_TIMEOUT_SECS: u64 = 15;
pub(crate) const IDLE_TIMEOUT_SECS: u64 = 90;

/// 明细日志开关：非空且非 0/false/off 时额外输出思考与可见文字预览。
const DETAIL_ENV: &str = "QC_AI_TRACE_DETAIL";
/// 明细预览的单字段字符上限，防止一段超长输出撑爆日志行。
const MAX_DETAIL_CHARS: usize = 2_000;
/// 工具名允许的最大字符数，超出即视为不可信输入。
const MAX_TOOL_NAME_CHARS: usize = 40;

/// 一次模型请求的生命周期只持有白名单标签与计数，取消时也不会泄露请求或响应原文。
pub(crate) struct RequestDiagnostics {
    trace: Uuid,
    tag: &'static str,
    protocol: &'static str,
    started: Instant,
    pub stage: &'static str,
    pub status: Option<u16>,
    pub turn: usize,
    pub retry: usize,
    pub streaming: bool,
    pub bytes: usize,
    pub chunks: usize,
    pub text_bytes: usize,
    pub events: usize,
    pub invalid_events: usize,
    pub done_marker: bool,
    pub connect_timeout_ms: u64,
    pub idle_timeout_ms: u64,
    pub tool_count: Option<usize>,
    shape: &'static str,
    finish_reason: &'static str,
    reasoning_seen: bool,
    tool_names: String,
    detail: bool,
    reasoning_preview: String,
    content_preview: String,
    finished: bool,
}

impl RequestDiagnostics {
    /// 关联身份仅接受规范化 UUID；日志前缀与协议标签由调用方提供固定值。
    pub fn new(trace: &str, tag: &'static str, protocol: &'static str) -> Self {
        Self {
            trace: Uuid::parse_str(trace).unwrap_or_else(|_| Uuid::now_v7()),
            tag,
            protocol,
            started: Instant::now(),
            stage: "build",
            status: None,
            turn: 0,
            retry: 0,
            streaming: false,
            bytes: 0,
            chunks: 0,
            text_bytes: 0,
            events: 0,
            invalid_events: 0,
            done_marker: false,
            connect_timeout_ms: CONNECT_TIMEOUT_SECS * 1000,
            idle_timeout_ms: IDLE_TIMEOUT_SECS * 1000,
            tool_count: None,
            shape: "empty",
            finish_reason: "none",
            reasoning_seen: false,
            tool_names: String::new(),
            detail: detail_enabled(),
            reasoning_preview: String::new(),
            content_preview: String::new(),
            finished: false,
        }
    }

    /// 写入轮次、重试与超时之后再记录起始行，保证同一条日志的关联信息完整。
    pub fn begin(&mut self) {
        self.log("start", "none", false);
    }

    /// 仅检查首个非空白字节，避免为诊断复制或再次反序列化整个正文。
    pub fn observe_chunk(&mut self, chunk: &[u8]) {
        self.chunks += 1;
        self.bytes += chunk.len();
        if self.shape == "empty" {
            if let Some(first) = chunk.iter().find(|byte| !byte.is_ascii_whitespace()) {
                self.shape = match first {
                    b'{' => "json_object_prefix",
                    b'[' => "json_array_prefix",
                    _ if self.streaming => "sse_declared",
                    _ => "other",
                };
            }
        }
    }

    /// 只观察固定结构与可见文字规模；明细开启时另存脱敏预览。
    pub fn observe_event(&mut self, event: &serde_json::Value) {
        self.events += 1;
        if let Some(text) = visible_text(event) {
            self.text_bytes += text.len();
            if self.detail {
                append_capped(&mut self.content_preview, text);
            }
        }
        if self.detail {
            if let Some(reasoning) = reasoning_text(event) {
                append_capped(&mut self.reasoning_preview, reasoning);
            }
        }
        let reason = event
            .pointer("/choices/0/finish_reason")
            .or_else(|| event.pointer("/delta/stop_reason"))
            .or_else(|| event.get("stop_reason"))
            .and_then(serde_json::Value::as_str);
        if let Some(reason) = reason {
            self.finish_reason = match reason {
                "stop" => "stop",
                "length" => "length",
                "tool_calls" => "tool_calls",
                "content_filter" => "content_filter",
                "end_turn" => "end_turn",
                "max_tokens" => "max_tokens",
                "tool_use" => "tool_use",
                "stop_sequence" => "stop_sequence",
                _ => "other",
            };
        }
        match event.get("type").and_then(serde_json::Value::as_str) {
            Some("response.completed") => self.finish_reason = "completed",
            Some("response.incomplete") => self.finish_reason = "incomplete",
            Some("response.failed") => self.finish_reason = "failed",
            Some("message_stop") => self.done_marker = true,
            _ => {}
        }
        self.reasoning_seen |= event
            .pointer("/choices/0/delta/reasoning_content")
            .is_some()
            || event.pointer("/choices/0/delta/reasoning").is_some()
            || event.pointer("/delta/thinking").is_some()
            || event
                .pointer("/item/type")
                .and_then(serde_json::Value::as_str)
                == Some("reasoning");
    }

    /// 记录本轮调用的工具数量与经过字符过滤的名字，未知形态折叠为 unknown。
    pub fn record_calls<'a>(&mut self, names: impl IntoIterator<Item = &'a str>) {
        let mut count = 0usize;
        let mut summary = String::new();
        for name in names {
            count += 1;
            if !summary.is_empty() {
                summary.push(',');
            }
            summary.push_str(safe_tool_label(name));
            if summary.len() >= MAX_TOOL_NAME_CHARS * 4 {
                summary.push_str(",...");
                break;
            }
        }
        self.tool_count = Some(count);
        self.tool_names = summary;
    }

    /// 阶段日志有界，不为每个流片段生成日志。
    pub fn checkpoint(&self, outcome: &'static str) {
        self.log(outcome, "none", false);
    }

    /// 成功结束以调用方的权威文本规模覆盖观察值，工具规模由 record_calls 提前写入。
    pub fn finish_success(&mut self, text_bytes: usize) {
        self.finished = true;
        self.text_bytes = text_bytes;
        self.log("success", "none", false);
    }

    /// 失败只输出白名单错误码，超时单独标注。
    pub fn finish_error(&mut self, error: &CommandError) {
        self.finished = true;
        self.log(
            "error",
            safe_error_code(error.code),
            error.code == "PROVIDER_TIMEOUT",
        );
    }

    /// 日志字段只能来自固定标签、规范 UUID、数值、布尔值与已过滤的工具名。
    fn log(&self, outcome: &'static str, code: &'static str, timeout: bool) {
        eprintln!("{}", self.line(outcome, code, timeout));
    }

    /// 单行格式也供测试验证脱敏边界，不能接受原始错误描述。
    fn line(&self, outcome: &'static str, code: &'static str, timeout: bool) -> String {
        let detail = if self.detail {
            format!(
                " reasoning={} content={}",
                quoted(&self.reasoning_preview),
                quoted(&self.content_preview)
            )
        } else {
            String::new()
        };
        format!(
            "[QuailCard][{} trace={} protocol={}] stage={} outcome={} turn={} retry={} status={:?} elapsed_ms={} connect_timeout_ms={} idle_timeout_ms={} timeout={} code={} streaming={} shape={} body_bytes={} chunks={} text_bytes={} tool_count={:?} tool_names=[{}] sse_events={} invalid_sse_events={} done_marker={} finish_reason={} reasoning_seen={}{}",
            self.tag,
            self.trace,
            self.protocol,
            self.stage,
            outcome,
            self.turn,
            self.retry,
            self.status,
            self.started.elapsed().as_millis(),
            self.connect_timeout_ms,
            self.idle_timeout_ms,
            timeout,
            code,
            self.streaming,
            self.shape,
            self.bytes,
            self.chunks,
            self.text_bytes,
            self.tool_count,
            self.tool_names,
            self.events,
            self.invalid_events,
            self.done_marker,
            self.finish_reason,
            self.reasoning_seen,
            detail,
        )
    }
}

impl Drop for RequestDiagnostics {
    /// 外层取消和外层超时无法区分，统一记录 dropped，不伪称网络超时。
    fn drop(&mut self) {
        if !self.finished {
            self.log("dropped", "none", false);
        }
    }
}

/// 明细开关只在进程内读取一次，避免每个流事件访问环境变量。
fn detail_enabled() -> bool {
    static ENABLED: OnceLock<bool> = OnceLock::new();
    *ENABLED.get_or_init(|| {
        std::env::var(DETAIL_ENV)
            .map(|value| {
                let value = value.trim().to_ascii_lowercase();
                !matches!(value.as_str(), "" | "0" | "false" | "off")
            })
            .unwrap_or(false)
    })
}

/// 工具名只保留自有 schema 允许的形态，未知或超长名字折叠为 unknown，避免日志注入。
fn safe_tool_label(name: &str) -> &str {
    let valid = !name.is_empty()
        && name.chars().count() <= MAX_TOOL_NAME_CHARS
        && name
            .chars()
            .all(|ch| ch.is_ascii_lowercase() || ch.is_ascii_digit() || ch == '_');
    if valid {
        name
    } else {
        "unknown"
    }
}

/// 未知错误码也可能包含自定义文本，必须收敛到固定 unknown。
fn safe_error_code(code: &str) -> &'static str {
    match code {
        "PROVIDER_TIMEOUT" => "PROVIDER_TIMEOUT",
        "PROVIDER_UNREACHABLE" => "PROVIDER_UNREACHABLE",
        "PROVIDER_REQUEST_FAILED" => "PROVIDER_REQUEST_FAILED",
        "PROVIDER_RESPONSE_TOO_LARGE" => "PROVIDER_RESPONSE_TOO_LARGE",
        "PROVIDER_RESPONSE_INVALID" => "PROVIDER_RESPONSE_INVALID",
        "PROVIDER_TOOL_RESPONSE_INVALID" => "PROVIDER_TOOL_RESPONSE_INVALID",
        "PROVIDER_TOOL_NOT_CALLED" => "PROVIDER_TOOL_NOT_CALLED",
        "PROVIDER_AUTH_FAILED" => "PROVIDER_AUTH_FAILED",
        "PROVIDER_ENDPOINT_NOT_FOUND" => "PROVIDER_ENDPOINT_NOT_FOUND",
        "PROVIDER_RATE_LIMITED" => "PROVIDER_RATE_LIMITED",
        "PROVIDER_OVERLOADED" => "PROVIDER_OVERLOADED",
        "PROVIDER_BAD_REQUEST" => "PROVIDER_BAD_REQUEST",
        "PROVIDER_SERVER_ERROR" => "PROVIDER_SERVER_ERROR",
        _ => "unknown",
    }
}

/// 只提取协议定义的可见文字增量，思考、工具参数与凭据回显不计入规模。
fn visible_text(event: &serde_json::Value) -> Option<&str> {
    let direct = event
        .pointer("/choices/0/delta/content")
        .and_then(serde_json::Value::as_str)
        .or_else(|| {
            event
                .pointer("/delta/text")
                .and_then(serde_json::Value::as_str)
        });
    if direct.is_some() {
        return direct.filter(|text| !text.is_empty());
    }
    let is_output_text =
        event.get("type").and_then(serde_json::Value::as_str) == Some("response.output_text.delta");
    is_output_text
        .then(|| event.get("delta").and_then(serde_json::Value::as_str))
        .flatten()
        .filter(|text| !text.is_empty())
}

/// 只提取协议定义的思考增量，供明细日志使用。
fn reasoning_text(event: &serde_json::Value) -> Option<&str> {
    event
        .pointer("/choices/0/delta/reasoning_content")
        .and_then(serde_json::Value::as_str)
        .or_else(|| {
            event
                .pointer("/choices/0/delta/reasoning")
                .and_then(serde_json::Value::as_str)
        })
        .or_else(|| {
            event
                .pointer("/delta/thinking")
                .and_then(serde_json::Value::as_str)
        })
        .filter(|text| !text.is_empty())
}

/// 明细预览按字符截断，避免单个超长增量绕过上限。
fn append_capped(buffer: &mut String, text: &str) {
    let remaining = MAX_DETAIL_CHARS.saturating_sub(buffer.chars().count());
    buffer.extend(text.chars().take(remaining));
}

/// 明细预览先脱敏再转义，让换行与引号都不能破坏日志行的结构。
fn quoted(text: &str) -> String {
    serde_json::to_string(&redact(text)).unwrap_or_else(|_| "\"\"".to_string())
}

/// 只处理最常见的凭据回显形态；明细日志是本地调试用途，不是通用脱敏。
fn redact(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut redact_next = false;
    for (index, word) in text.split_whitespace().enumerate() {
        if index > 0 {
            out.push(' ');
        }
        let lower = word.to_ascii_lowercase();
        if redact_next || looks_secret(&lower) {
            out.push_str("[redacted]");
            redact_next = false;
        } else {
            out.push_str(word);
            redact_next = lower == "bearer";
        }
    }
    out
}

/// 凭据前缀只做形态匹配，不尝试识别具体供应商。
fn looks_secret(lower: &str) -> bool {
    lower.starts_with("sk-")
        || lower.starts_with("api_key=")
        || lower.starts_with("access_token=")
        || lower.starts_with("authorization:")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    /// 自定义身份、正文和错误即使含换行及凭据，也不能进入诊断行。
    fn diagnostics_reject_custom_strings() {
        let secret = "https://private.invalid/token?key=secret\ninjected";
        let mut diagnostic = RequestDiagnostics::new(secret, "CardGen", "openai_chat");
        diagnostic.observe_chunk(secret.as_bytes());
        let line = diagnostic.line("error", safe_error_code(secret), false);
        assert!(!line.contains(secret));
        assert!(!line.contains("private.invalid"));
        assert!(!line.contains('\n'));
        assert!(Uuid::parse_str(&diagnostic.trace.to_string()).is_ok());
        assert!(line.contains("code=unknown"));
        diagnostic.finished = true;
    }

    #[test]
    /// 思考字段只记录存在性，供应商自定义结束原因不能作为日志文本。
    fn diagnostics_allowlist_finish_reason_and_reasoning() {
        let secret = "private response and credentials";
        let mut diagnostic = RequestDiagnostics::new("", "CardGen", "openai_chat");
        diagnostic.observe_event(&serde_json::json!({"choices":[{
            "delta":{"reasoning_content":secret}, "finish_reason":secret
        }]}));
        let line = diagnostic.line("success", "none", false);
        assert!(line.contains("finish_reason=other reasoning_seen=true"));
        assert!(!line.contains(secret));
        diagnostic.observe_event(&serde_json::json!({"choices":[{"finish_reason":"length"}]}));
        assert_eq!(diagnostic.finish_reason, "length");
        assert_eq!(diagnostic.events, 2);
        diagnostic.finished = true;
    }

    #[test]
    /// 成功但空文字仍有明确的零计数，UUID 关联不依赖原始输入格式。
    fn diagnostics_record_empty_reply_shape() {
        let id = Uuid::now_v7();
        let mut diagnostic = RequestDiagnostics::new(&id.to_string(), "CardGen", "openai_chat");
        diagnostic.status = Some(200);
        diagnostic.observe_chunk(b" ");
        diagnostic.observe_chunk(br#"{"choices":[]}"#);
        diagnostic.record_calls(std::iter::empty::<&str>());
        diagnostic.finish_success(0);
        let line = diagnostic.line("success", "none", false);
        assert!(line.contains(&format!("trace={id}")));
        assert!(line.contains("shape=json_object_prefix"));
        assert!(line.contains("text_bytes=0 tool_count=Some(0) tool_names=[]"));
        assert!(line.contains("connect_timeout_ms=15000 idle_timeout_ms=90000"));
        assert_eq!(diagnostic.chunks, 2);
    }

    #[test]
    /// 只有可见文字进入 text_bytes，思考与工具参数不能抬高文本规模。
    fn counts_only_visible_text() {
        let mut diagnostic = RequestDiagnostics::new("", "CardGen", "openai_chat");
        diagnostic.observe_event(&serde_json::json!({"choices":[{
            "delta":{"reasoning_content":"secret","content":"你好"}
        }]}));
        diagnostic.observe_event(&serde_json::json!({
            "type":"response.reasoning_summary_text.delta","delta":"secret"
        }));
        assert_eq!(diagnostic.text_bytes, "你好".len());
        diagnostic.finished = true;
    }

    #[test]
    /// 工具名与明细预览都经过过滤，凭据形态与注入字符不会原样进入日志行。
    fn filters_tool_names_and_redacts_detail() {
        let long = "x".repeat(80);
        let mut diagnostic = RequestDiagnostics::new("", "CardGen", "openai_chat");
        diagnostic.detail = true;
        diagnostic.record_calls(["emit_card", "bad name\ninjected", long.as_str()]);
        diagnostic.observe_event(&serde_json::json!({"choices":[{
            "delta":{"reasoning_content":"use Bearer sk-live-secret and finish"}
        }]}));
        let line = diagnostic.line("success", "none", false);
        assert!(line.contains("tool_names=[emit_card,unknown,unknown]"));
        assert!(!line.contains("sk-live-secret"));
        assert!(!line.contains("Bearer sk-"));
        assert!(line.contains("[redacted]"));
        assert!(!line.contains('\n'));
        diagnostic.finished = true;
    }
}
