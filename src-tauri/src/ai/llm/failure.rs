//! 供应商中立失败事实与稳定错误码；调用方只匹配 code，绝不匹配 message 文本。

/// 稳定错误码；新增码必须同步 safe 投影与重试集合。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum FailureCode {
    NoAdapter,
    DuplicateAdapter,
    MissingCredential,
    Auth,
    RateLimit,
    Quota,
    Overloaded,
    ContextWindowExceeded,
    Server,
    Timeout,
    Transport,
    Aborted,
    EmptyResponse,
    ToolNotCalled,
    ResponseInvalid,
    ResponseIncomplete,
    Unknown,
}

impl FailureCode {
    /// 日志与前端只接收稳定字符串，不接受供应商原文。
    pub(crate) const fn as_str(self) -> &'static str {
        match self {
            Self::NoAdapter => "NO_ADAPTER",
            Self::DuplicateAdapter => "DUPLICATE_ADAPTER",
            Self::MissingCredential => "MISSING_CREDENTIAL",
            Self::Auth => "AUTH",
            Self::RateLimit => "RATE_LIMIT",
            Self::Quota => "QUOTA",
            Self::Overloaded => "OVERLOADED",
            Self::ContextWindowExceeded => "CONTEXT_WINDOW_EXCEEDED",
            Self::Server => "SERVER",
            Self::Timeout => "TIMEOUT",
            Self::Transport => "TRANSPORT",
            Self::Aborted => "ABORTED",
            Self::EmptyResponse => "EMPTY_RESPONSE",
            Self::ToolNotCalled => "TOOL_NOT_CALLED",
            Self::ResponseInvalid => "RESPONSE_INVALID",
            Self::ResponseIncomplete => "RESPONSE_INCOMPLETE",
            Self::Unknown => "UNKNOWN",
        }
    }

    /// 默认可重试集合与供应商无关；策略层可覆盖。
    pub(crate) const fn retryable(self) -> bool {
        matches!(
            self,
            Self::EmptyResponse
                | Self::RateLimit
                | Self::Overloaded
                | Self::Server
                | Self::Timeout
                | Self::Transport
        )
    }
}

/// 可序列化的失败事实；message 只允许安全文案，禁止供应商原文或凭据。
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct LlmFailure {
    pub message: String,
    pub code: FailureCode,
    pub status: Option<u16>,
    pub retry_after_ms: Option<u64>,
    pub request_id: Option<String>,
}

impl LlmFailure {
    /// 构造供应商失败；message 由调用方提供安全文案。
    pub(crate) fn provider(code: FailureCode, message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            code,
            status: None,
            retry_after_ms: None,
            request_id: None,
        }
    }

    /// 兼容旧 CommandError 匹配的最小投影。
    pub(crate) fn is_retryable(&self) -> bool {
        self.code.retryable()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    /// 只有固定的传输类错误默认可重试，鉴权与配额不属于重试集合。
    fn retryable_codes_are_transport_only() {
        assert!(FailureCode::Timeout.retryable());
        assert!(FailureCode::EmptyResponse.retryable());
        assert!(!FailureCode::Auth.retryable());
        assert!(!FailureCode::Quota.retryable());
        assert!(!FailureCode::ToolNotCalled.retryable());
    }

    #[test]
    /// 错误码字符串稳定且大写，可用于前端与日志。
    fn codes_are_stable_uppercase() {
        assert_eq!(
            FailureCode::ContextWindowExceeded.as_str(),
            "CONTEXT_WINDOW_EXCEEDED"
        );
        assert_eq!(FailureCode::Unknown.as_str(), "UNKNOWN");
    }
}
