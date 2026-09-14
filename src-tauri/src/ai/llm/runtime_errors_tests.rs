//! Retry-After 与 HTTP 错误分类的确定性回归测试。

use super::*;
use reqwest::header::HeaderValue;

#[test]
/// 保留协议层已解析的毫秒提示，同时将配额耗尽与瞬态限流区分。
fn preserves_failure_retry_hint_and_quota_semantics() {
    let mut failure = LlmFailure::provider(FailureCode::RateLimit, "供应商限流");
    failure.retry_after_ms = Some(2300);
    let error = command_error(failure);
    assert_eq!(error.code, "PROVIDER_RATE_LIMITED");
    assert_eq!(error.retry_after_ms, Some(2300));
    let quota = command_error(LlmFailure::provider(FailureCode::Quota, "供应商配额耗尽"));
    assert_eq!(quota.code, "PROVIDER_QUOTA_EXCEEDED");
    assert_eq!(quota.retry_after_ms, None);
    let serialized = serde_json::to_value(error).unwrap();
    assert_eq!(
        serialized,
        serde_json::json!({"code": "PROVIDER_RATE_LIMITED", "message": "供应商限流"})
    );
}

/// 固定时间消除测试对本机时钟和时区的依赖。
fn reference_time() -> SystemTime {
    UNIX_EPOCH + Duration::from_secs(784111777)
}

#[test]
/// 秒数换算保留零值与最大安全值，拒绝符号、小数、负数及溢出。
fn parses_delta_seconds_without_overflow() {
    for (value, expected) in [
        ("0", Some(0)),
        ("120", Some(120000)),
        ("  001	", Some(1000)),
        ("18446744073709551", Some(18446744073709551000)),
        ("18446744073709552", None),
        ("18446744073709551616", None),
        ("", None),
        (" ", None),
        ("+1", None),
        ("-1", None),
        ("1.5", None),
        ("1e3", None),
        ("1, 2", None),
        ("１２", None),
    ] {
        assert_eq!(
            parse_retry_after(value, reference_time()),
            expected,
            "{value}"
        );
    }
}

#[test]
/// 三种 HTTP 日期形式应得到相同等待时间，且只输出毫秒数。
fn parses_http_date_formats() {
    for value in [
        "Sun, 06 Nov 1994 08:49:39 GMT",
        "Sunday, 06-Nov-94 08:49:39 GMT",
        "Sun Nov  6 08:49:39 1994",
    ] {
        assert_eq!(
            parse_retry_after(value, reference_time()),
            Some(2000),
            "{value}"
        );
    }
    assert_eq!(
        parse_retry_after("Sun, 06 Nov 1994 08:49:37 GMT", reference_time()),
        Some(0)
    );
    assert_eq!(
        parse_retry_after("Sun, 06 Nov 1994 08:49:36 GMT", reference_time()),
        Some(0)
    );
    assert_eq!(
        parse_retry_after(
            "Sun, 06 Nov 1994 08:49:39 GMT",
            reference_time() + Duration::from_millis(250)
        ),
        Some(1750)
    );
}

#[test]
/// 非法时区、字段范围、非闰年二月和无效日期不生成重试提示。
fn rejects_invalid_http_dates() {
    for value in [
        "Sun, 06 Nov 1994 08:49:39 UTC",
        "Sun, 06 Nov 1994 08:49:39 +0000",
        "Sun, 32 Nov 1994 08:49:39 GMT",
        "Sun, 00 Nov 1994 08:49:39 GMT",
        "Sun, 06 Xxx 1994 08:49:39 GMT",
        "Sun, 06 Nov 1994 24:49:39 GMT",
        "Sun, 06 Nov 1994 08:60:39 GMT",
        "Sun, 06 Nov 1994 08:49:60 GMT",
        "Sun, 29 Feb 2100 08:49:39 GMT",
        "Sun, 29 Feb 2023 08:49:39 GMT",
        "Sun, 06 Nov 94 08:49:39 GMT",
        "Sun, 06 Nov 1994 8:49:39 GMT",
        "Invalid, 06 Nov 1994 08:49:39 GMT",
        "Sun, 006 Nov 1994 08:49:39 GMT",
        "Sun, 06 Nov 0000 08:49:39 GMT",
        "Sun, 06 Nov 1994 08:49:39 GMT extra",
    ] {
        assert_eq!(parse_retry_after(value, reference_time()), None, "{value}");
    }
    assert!(parse_retry_after("Tue, 29 Feb 2000 08:49:39 GMT", reference_time()).is_some());
    assert!(parse_retry_after("Thu, 29 Feb 2024 08:49:39 GMT", reference_time()).is_some());
}

#[test]
/// 旧式两位年份超过当前年五十年时应解释为上一世纪。
fn rolls_obsolete_years_back_after_fifty_years() {
    assert_eq!(
        parse_retry_after("Sunday, 06-Nov-45 08:49:39 GMT", reference_time()),
        Some(0)
    );
    assert_eq!(
        parse_http_date("Sunday, 06-Nov-44 08:49:39 GMT", 784111777),
        parse_http_date("Sun, 06 Nov 2044 08:49:39 GMT", 784111777)
    );
}

#[test]
/// 只有 429 与 503 读取等待提示，缺失或非文本头始终忽略。
fn limits_retry_after_to_selected_statuses() {
    let header = HeaderValue::from_static("3");
    for status in [429, 503] {
        assert_eq!(
            retry_after(status, Some(&header), reference_time()),
            Some(3000)
        );
        assert_eq!(retry_after(status, None, reference_time()), None);
        let binary = HeaderValue::from_bytes(&[0xff]).unwrap();
        assert_eq!(retry_after(status, Some(&binary), reference_time()), None);
    }
    for status in [200, 400, 401, 403, 404, 408, 422, 500, 502, 504] {
        assert_eq!(retry_after(status, Some(&header), reference_time()), None);
    }
}

#[test]
/// 认证与普通客户端拒绝优先，供应商正文不能把它们改写为过载重试。
fn preserves_authentication_and_client_error_statuses() {
    for body in [
        br#"{"error":{"code":"server_is_overloaded"}}"#.as_slice(),
        br#"{"error":{"type":"service_unavailable_error"}}"#.as_slice(),
    ] {
        for (status, expected) in [
            (401, "PROVIDER_AUTH_FAILED"),
            (403, "PROVIDER_AUTH_FAILED"),
            (400, "PROVIDER_TOOL_UNSUPPORTED"),
            (422, "PROVIDER_TOOL_UNSUPPORTED"),
            (404, "PROVIDER_ENDPOINT_NOT_FOUND"),
            (409, "PROVIDER_REQUEST_REJECTED"),
            (429, "PROVIDER_OVERLOADED"),
            (503, "PROVIDER_OVERLOADED"),
        ] {
            assert_eq!(map_provider_body_error(status, body).code, expected);
        }
    }
}

#[test]
/// 普通限流及服务错误保留既有错误码，敏感正文不进入固定消息或日志摘要。
fn maps_status_without_echoing_provider_content() {
    let body = br#"{"error":{"message":"secret-api-key","code":"secret-api-key"}}"#;
    for (status, expected) in [
        (429, "PROVIDER_RATE_LIMITED"),
        (503, "PROVIDER_SERVER_ERROR"),
        (401, "PROVIDER_AUTH_FAILED"),
        (500, "PROVIDER_SERVER_ERROR"),
    ] {
        let error = map_provider_body_error(status, body);
        assert_eq!(error.code, expected);
        assert!(!error.message.contains("secret-api-key"));
    }
    assert!(!provider_error_summary(body).contains("secret-api-key"));
    assert_eq!(
        map_provider_body_error(503, b"not json").code,
        "PROVIDER_SERVER_ERROR"
    );
}
