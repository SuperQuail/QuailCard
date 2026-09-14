//! HTTP 与协议失败的安全映射；原始响应头、正文不进入错误 DTO。

#[cfg(test)]
#[path = "runtime_errors_tests.rs"]
mod tests;

use std::time::{Duration, SystemTime, UNIX_EPOCH};

use reqwest::{header::RETRY_AFTER, Response};
use serde_json::Value;

use super::super::failure::{FailureCode, LlmFailure};
use super::MAX_RESPONSE_BYTES;
use crate::error::CommandError;

/// 失败事实转安全错误；配额耗尽与限流分码，避免把不可重试失败当作瞬态错误。
pub(super) fn command_error(failure: LlmFailure) -> CommandError {
    let code = match failure.code {
        FailureCode::Timeout => "PROVIDER_TIMEOUT",
        FailureCode::Auth => "PROVIDER_AUTH_FAILED",
        FailureCode::RateLimit => "PROVIDER_RATE_LIMITED",
        FailureCode::Quota => "PROVIDER_QUOTA_EXCEEDED",
        FailureCode::Overloaded => "PROVIDER_OVERLOADED",
        FailureCode::ContextWindowExceeded => "PROVIDER_BAD_REQUEST",
        FailureCode::Server => "PROVIDER_SERVER_ERROR",
        FailureCode::Transport | FailureCode::Aborted | FailureCode::Unknown => {
            "PROVIDER_REQUEST_FAILED"
        }
        FailureCode::MissingCredential => "PROVIDER_CREDENTIAL_MISSING",
        FailureCode::EmptyResponse | FailureCode::ToolNotCalled => "PROVIDER_TOOL_NOT_CALLED",
        FailureCode::ResponseInvalid => "PROVIDER_RESPONSE_INVALID",
        FailureCode::ResponseIncomplete => "PROVIDER_RESPONSE_INCOMPLETE",
        FailureCode::NoAdapter => "NO_ADAPTER",
        FailureCode::DuplicateAdapter => "DUPLICATE_ADAPTER",
    };
    CommandError::provider(code, failure.message).with_retry_after(failure.retry_after_ms)
}

/// 将流读取网络错误转换为统一供应商错误。
pub(super) fn map_stream_error(error: reqwest::Error) -> CommandError {
    if error.is_timeout() {
        CommandError::provider("PROVIDER_TIMEOUT", "模型请求超时，请检查网络或稍后重试")
    } else {
        CommandError::provider("PROVIDER_REQUEST_FAILED", "读取模型流式响应失败")
    }
}

/// 无法识别的响应结构。
pub(super) fn invalid_response() -> CommandError {
    CommandError::provider("PROVIDER_RESPONSE_INVALID", "供应商返回了无法识别的响应")
}

/// 空闲看门狗超时：只在超过 idle 没有任何新数据时触发，与整请求总时长无关。
pub(super) fn idle_timeout() -> CommandError {
    CommandError::provider(
        "PROVIDER_TIMEOUT",
        "模型响应长时间没有新数据，请检查网络或稍后重试",
    )
}

/// 流在正常终止前中断。
pub(super) fn incomplete() -> CommandError {
    CommandError::provider(
        "PROVIDER_RESPONSE_INCOMPLETE",
        "供应商流式响应在正常结束前中断",
    )
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

/// 错误正文只用于受限分类，供应商可能回显凭据，不能直接进入日志或 DTO。
pub(super) async fn map_body_error(
    status: u16,
    response: Response,
    idle: Duration,
) -> CommandError {
    let retry_after_ms = retry_after(
        status,
        response.headers().get(RETRY_AFTER),
        SystemTime::now(),
    );
    let body = read_limited_body(response, idle).await.unwrap_or_default();
    eprintln!(
        "[QuailCard] 供应商请求失败（HTTP {status}）：{}",
        provider_error_summary(&body)
    );
    map_provider_body_error(status, &body).with_retry_after(retry_after_ms)
}

/// 只为限流与服务不可用保存毫秒数；响应头借用在读取正文前结束。
fn retry_after(
    status: u16,
    header: Option<&reqwest::header::HeaderValue>,
    now: SystemTime,
) -> Option<u64> {
    if !matches!(status, 429 | 503) {
        return None;
    }
    parse_retry_after(header?.to_str().ok()?, now)
}

/// 秒数只接受无符号十进制；过期日期归零，非法值与溢出不提供等待提示。
fn parse_retry_after(value: &str, now: SystemTime) -> Option<u64> {
    let value = value.trim_matches([' ', '\t']);
    if !value.is_empty() && value.bytes().all(|byte| byte.is_ascii_digit()) {
        return value.parse::<u64>().ok()?.checked_mul(1000);
    }
    let now_ms = i128::try_from(now.duration_since(UNIX_EPOCH).ok()?.as_millis()).ok()?;
    let date_ms = i128::from(parse_http_date(value, now_ms / 1000)?) * 1000;
    u64::try_from((date_ms - now_ms).max(0)).ok()
}

/// 接受 HTTP 的 IMF-fixdate、旧 RFC 850 和 asctime 形式，拒绝非 GMT 时区。
fn parse_http_date(value: &str, now_seconds: i128) -> Option<i64> {
    let fields: Vec<_> = value.split_ascii_whitespace().collect();
    let (weekday, day, month, year, clock) = match fields.as_slice() {
        [weekday, day, month, year, clock, "GMT"] => (
            weekday.strip_suffix(',')?,
            *day,
            *month,
            decimal(year, 4)?,
            *clock,
        ),
        [weekday, date, clock, "GMT"] => {
            let parts: Vec<_> = date.split('-').collect();
            let [day, month, year] = parts.as_slice() else {
                return None;
            };
            let mut current_year = 1970;
            while current_year < 9999 && year_start(current_year + 1) <= now_seconds {
                current_year += 1;
            }
            let mut year = (current_year + 50) / 100 * 100 + decimal(year, 2)?;
            if year > current_year + 50 {
                year -= 100;
            }
            (weekday.strip_suffix(',')?, *day, *month, year, *clock)
        }
        [weekday, month, day, clock, year] => (*weekday, *day, *month, decimal(year, 4)?, *clock),
        _ => return None,
    };
    if ![
        "Mon",
        "Tue",
        "Wed",
        "Thu",
        "Fri",
        "Sat",
        "Sun",
        "Monday",
        "Tuesday",
        "Wednesday",
        "Thursday",
        "Friday",
        "Saturday",
        "Sunday",
    ]
    .contains(&weekday)
    {
        return None;
    }
    let month = [
        "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
    ]
    .iter()
    .position(|name| *name == month)?;
    let day_width = day.len();
    let day = decimal(day, day_width)?;
    let days = [
        31,
        if is_leap_year(year) { 29 } else { 28 },
        31,
        30,
        31,
        30,
        31,
        31,
        30,
        31,
        30,
        31,
    ];
    if !(1601..=9999).contains(&year)
        || !(1..=2).contains(&day_width)
        || day == 0
        || day > days[month]
    {
        return None;
    }
    let clock: Vec<_> = clock.split(':').collect();
    let [hour, minute, second] = clock.as_slice() else {
        return None;
    };
    let (hour, minute, second) = (decimal(hour, 2)?, decimal(minute, 2)?, decimal(second, 2)?);
    if hour > 23 || minute > 59 || second > 59 {
        return None;
    }
    let days_before_month: u32 = days[..month].iter().sum();
    let seconds = year_start(year)
        + i128::from(days_before_month + day - 1) * 86400
        + i128::from(hour * 3600 + minute * 60 + second);
    i64::try_from(seconds).ok()
}

/// 只接受定宽 ASCII 数字，避免日期中的符号及超长字段被宽松解析。
fn decimal(value: &str, width: usize) -> Option<u32> {
    if value.is_empty() || value.len() != width || !value.bytes().all(|byte| byte.is_ascii_digit())
    {
        return None;
    }
    value.parse().ok()
}

/// 使用公历闰年规则校验二月，避免日期溢出归一化成另一天。
fn is_leap_year(year: u32) -> bool {
    year.is_multiple_of(4) && (!year.is_multiple_of(100) || year.is_multiple_of(400))
}

/// 按公历直接求年初相对 Unix 纪元的秒数，无需新增日期依赖。
fn year_start(year: u32) -> i128 {
    let previous = i128::from(year) - 1;
    (previous * 365 + previous / 4 - previous / 100 + previous / 400 - 719162) * 86400
}

/// 逐块读取响应并阻止无界内存增长。
async fn read_limited_body(
    mut response: Response,
    idle: Duration,
) -> Result<Vec<u8>, CommandError> {
    let mut body = Vec::new();
    loop {
        let chunk = match tokio::time::timeout(idle, response.chunk()).await {
            Ok(Ok(Some(chunk))) => chunk,
            Ok(Ok(None)) => break,
            Ok(Err(error)) => return Err(map_request_error(error)),
            Err(_) => return Err(idle_timeout()),
        };
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

/// 日志只保留大小和编码形态，连供应商自定义 code 都可能包含敏感原文。
fn provider_error_summary(body: &[u8]) -> String {
    format!(
        "bytes={} json={}",
        body.len(),
        serde_json::from_slice::<Value>(body).is_ok()
    )
}

/// 只识别固定的过载标识，其余错误由 HTTP 状态映射为安全消息。
fn map_provider_body_error(status: u16, body: &[u8]) -> CommandError {
    // 鉴权和请求拒绝必须以 HTTP 状态为准，不能被正文伪装为可重试过载。
    if (400..500).contains(&status) && status != 429 {
        return map_status_error(status);
    }
    let value = serde_json::from_slice::<Value>(body).unwrap_or(Value::Null);
    if value.pointer("/error/code").and_then(Value::as_str) == Some("server_is_overloaded")
        || value.pointer("/error/type").and_then(Value::as_str) == Some("service_unavailable_error")
    {
        CommandError::provider("PROVIDER_OVERLOADED", "模型服务当前负载过高，请稍后重试")
    } else {
        map_status_error(status)
    }
}
