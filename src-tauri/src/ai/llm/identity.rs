//! 模型请求的客户端身份：专属 User-Agent 与 OpenCode 会话请求头。

use reqwest::RequestBuilder;

/// 官方要求客户端使用自身专属标识，而不是通用 SDK 或 HTTP 库名称。
pub(crate) const USER_AGENT: &str = concat!("QuailCard/", env!("CARGO_PKG_VERSION"));

/// OpenCode 用该请求头关联同一段对话，影响路由与提示词缓存。
pub(crate) const SESSION_HEADER: &str = "x-opencode-session";

/// 为模型请求附加专属 User-Agent；OpenCode 端点再附加稳定的会话 ID。
pub(crate) fn apply_client_identity(
    builder: RequestBuilder,
    base_url: &str,
    session_id: &str,
) -> RequestBuilder {
    let builder = builder.header("User-Agent", USER_AGENT);
    if session_id.trim().is_empty() || !is_opencode_endpoint(base_url) {
        return builder;
    }
    builder.header(SESSION_HEADER, session_id)
}

/// 只认官方域名及其子域，自定义反代地址不主动附加 OpenCode 请求头。
fn is_opencode_endpoint(base_url: &str) -> bool {
    reqwest::Url::parse(base_url)
        .ok()
        .and_then(|url| url.host_str().map(str::to_ascii_lowercase))
        .is_some_and(|host| host == "opencode.ai" || host.ends_with(".opencode.ai"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use reqwest::header;

    /// 构造最小请求以检查最终请求头。
    fn request(base_url: &str, session_id: &str) -> reqwest::Request {
        apply_client_identity(reqwest::Client::new().post(base_url), base_url, session_id)
            .build()
            .expect("构造身份请求失败")
    }

    #[test]
    /// 专属 UA 对所有供应商生效，会话头只发往 OpenCode 官方域名。
    fn applies_user_agent_and_scopes_session() {
        let opencode = request(
            "https://opencode.ai/zen/go/v1/chat/completions",
            "session-1",
        );
        assert_eq!(
            opencode.headers().get(header::USER_AGENT).unwrap(),
            USER_AGENT
        );
        assert_eq!(opencode.headers().get(SESSION_HEADER).unwrap(), "session-1");

        let other = request("https://example.com/v1/chat/completions", "session-1");
        assert_eq!(other.headers().get(header::USER_AGENT).unwrap(), USER_AGENT);
        assert!(other.headers().get(SESSION_HEADER).is_none());
    }

    #[test]
    /// 子域算官方端点；相似域名和空会话 ID 都不能带上会话头。
    fn rejects_lookalike_hosts_and_empty_session() {
        assert!(is_opencode_endpoint("https://zen.opencode.ai/v1"));
        assert!(!is_opencode_endpoint("https://opencode.ai.evil.com/v1"));
        assert!(!is_opencode_endpoint("not a url"));

        let subdomain = request("https://zen.opencode.ai/v1/messages", "session-2");
        assert_eq!(
            subdomain.headers().get(SESSION_HEADER).unwrap(),
            "session-2"
        );
        let empty = request("https://opencode.ai/zen/go/v1/chat/completions", "  ");
        assert!(empty.headers().get(SESSION_HEADER).is_none());
    }
}
