use super::super::ToolDefinition;
use super::*;
use crate::models::GenerationImage;
use reqwest::header;
use serde_json::json;
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpListener,
};

/// 创建协议请求测试使用的供应商配置。
fn test_config(protocol: &str, base_url: &str) -> ProviderConfig {
    ProviderConfig {
        id: "provider".to_string(),
        protocol: protocol.to_string(),
        model: "test-model".to_string(),
        base_url: base_url.to_string(),
        secret_ref: None,
        auth_type: None,
        oauth_account_id: None,
        provider_type: "api".to_string(),
        supports_vision: false,
    }
}

/// 创建协议请求测试使用的工具定义。
fn test_tool() -> ToolDefinition {
    ToolDefinition {
        name: "emit_result",
        description: "输出测试结果",
        input_schema: json!({
            "type": "object",
            "properties": { "value": { "type": "string" } },
            "required": ["value"],
            "additionalProperties": false
        }),
    }
}

/// 创建多模态请求测试使用的最小图片。
fn test_image() -> GenerationImage {
    GenerationImage {
        name: "note.png".to_string(),
        mime_type: "image/png".to_string(),
        data_base64: "aW1hZ2U=".to_string(),
    }
}

/// 读取请求中的 JSON Body。
fn request_body(request: &Request) -> Value {
    serde_json::from_slice(
        request
            .body()
            .and_then(reqwest::Body::as_bytes)
            .expect("请求缺少可读取 Body"),
    )
    .expect("请求 Body 不是 JSON")
}

/// 启动最小 HTTP 服务，连续返回不含工具调用的 OpenAI SSE 响应。
async fn serve_missing_tool_stream(listener: TcpListener, response_count: usize) {
    let body = concat!(
        "data: {\"choices\":[{\"delta\":{\"content\":\"没有调用工具\"},",
        "\"finish_reason\":\"stop\"}]}\n\ndata: [DONE]\n\n"
    );
    let response = format!(
        "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
        body.len(),
        body
    );
    for _ in 0..response_count {
        let (mut socket, _) = listener.accept().await.expect("接收测试请求失败");
        let mut request = Vec::new();
        let mut buffer = [0_u8; 2048];
        loop {
            let count = socket.read(&mut buffer).await.expect("读取测试请求失败");
            if count == 0 {
                break;
            }
            request.extend_from_slice(&buffer[..count]);
            if request.windows(4).any(|window| window == b"\r\n\r\n") {
                break;
            }
        }
        socket
            .write_all(response.as_bytes())
            .await
            .expect("写入测试响应失败");
    }
}

#[test]
/// OpenAI 请求使用版本端点和 Bearer 认证。
fn builds_openai_request() {
    let client = AiClient::new().expect("创建客户端失败");
    let tool = test_tool();
    let tool_request = ToolRequest {
        trace_id: "trace",
        turn: 1,
        system_prompt: "system",
        user_prompt: "user",
        images: &[],
        tool: &tool,
        max_tokens: 100,
        timeout: Duration::from_secs(5),
    };
    let request = client
        .build_tool_request(
            &test_config("OpenAI Compatible", "https://example.com/v1"),
            "secret",
            &tool_request,
            false,
        )
        .expect("构造请求失败");
    assert_eq!(
        request.url().as_str(),
        "https://example.com/v1/chat/completions"
    );
    assert_eq!(
        request.headers().get(header::AUTHORIZATION).unwrap(),
        "Bearer secret"
    );
    let body = request_body(&request);
    assert_eq!(body["tools"][0]["function"]["name"], "emit_result");
    assert_eq!(body["tools"][0]["function"]["strict"], true);
    assert!(body.get("tool_choice").is_none());
    assert_eq!(body["parallel_tool_calls"], false);
    assert_eq!(body["stream"], true);
}

#[test]
/// Anthropic 请求不会重复拼接 v1 路径。
fn builds_anthropic_request() {
    let client = AiClient::new().expect("创建客户端失败");
    let tool = test_tool();
    let tool_request = ToolRequest {
        trace_id: "trace",
        turn: 1,
        system_prompt: "system",
        user_prompt: "user",
        images: &[],
        tool: &tool,
        max_tokens: 100,
        timeout: Duration::from_secs(5),
    };
    let request = client
        .build_tool_request(
            &test_config("Anthropic Messages", "https://example.com/v1/"),
            "secret",
            &tool_request,
            false,
        )
        .expect("构造请求失败");
    assert_eq!(request.url().as_str(), "https://example.com/v1/messages");
    assert_eq!(request.headers().get("x-api-key").unwrap(), "secret");
    let body = request_body(&request);
    assert_eq!(body["tools"][0]["name"], "emit_result");
    assert_eq!(body["tool_choice"]["type"], "tool");
    assert_eq!(body["tool_choice"]["name"], "emit_result");
    assert_eq!(body["stream"], true);
}

#[test]
/// OAuth 请求固定使用 Codex Responses 端点和账号路由头。
fn builds_openai_oauth_request() {
    let client = AiClient::new().expect("创建客户端失败");
    let tool = test_tool();
    let mut config = test_config("OpenAI Compatible", "https://ignored.example/v1");
    config.provider_type = OPENAI_SUBSCRIPTION_PROVIDER_TYPE.to_string();
    let request = client
        .build_openai_oauth_request(
            &config,
            "oauth-access",
            Some("account-1"),
            &ToolRequest {
                trace_id: "trace",
                turn: 1,
                system_prompt: "system",
                user_prompt: "user",
                images: &[],
                tool: &tool,
                max_tokens: 100,
                timeout: Duration::from_secs(5),
            },
            false,
        )
        .expect("构造 OAuth 请求失败");
    assert_eq!(request.url().as_str(), OPENAI_SUBSCRIPTION_ENDPOINT);
    assert_eq!(
        request.headers().get(header::AUTHORIZATION).unwrap(),
        "Bearer oauth-access"
    );
    assert_eq!(
        request.headers().get("ChatGPT-Account-Id").unwrap(),
        "account-1"
    );
    let body = request_body(&request);
    assert!(body["input"].is_array());
    assert_eq!(body["input"][0]["role"], "user");
    assert_eq!(body["input"][0]["content"][0]["type"], "input_text");
    assert_eq!(body["input"][0]["content"][0]["text"], "user");
    assert_eq!(body["tools"][0]["name"], "emit_result");
    assert_eq!(body["tools"][0]["strict"], true);
    assert_eq!(body["tool_choice"]["name"], "emit_result");
    assert_eq!(body["store"], false);
    assert_eq!(body["stream"], true);
    assert!(body.get("max_output_tokens").is_none());
}

#[test]
/// 三种协议会使用各自要求的图片内容格式。
fn builds_multimodal_request_bodies() {
    let tool = test_tool();
    let images = vec![test_image()];
    let request = ToolRequest {
        trace_id: "trace",
        turn: 1,
        system_prompt: "system",
        user_prompt: "user",
        images: &images,
        tool: &tool,
        max_tokens: 100,
        timeout: Duration::from_secs(5),
    };
    let openai = openai_body(
        &test_config("OpenAI Compatible", "https://example.com/v1"),
        &request,
        false,
    );
    let responses = openai_responses_body(
        &test_config("OpenAI Compatible", "https://example.com/v1"),
        &request,
        false,
    );
    let anthropic = anthropic_body(
        &test_config("Anthropic Messages", "https://example.com/v1"),
        &request,
        false,
    );
    assert_eq!(openai["messages"][1]["content"][1]["type"], "image_url");
    assert_eq!(responses["input"][0]["content"][1]["type"], "input_image");
    assert_eq!(
        anthropic["messages"][0]["content"][1]["source"]["media_type"],
        "image/png"
    );
}

#[test]
/// 多工具请求允许 OpenAI 并行调用且 Anthropic 不禁用并行工具。
fn enables_parallel_multi_tool_calls() {
    let tools = vec![test_tool()];
    let request = MultiToolRequest {
        trace_id: "trace",
        turn: 1,
        system_prompt: "system",
        user_prompt: "user",
        images: &[],
        tools: &tools,
        history: &[],
        max_tokens: 100,
        timeout: Duration::from_secs(5),
    };
    let openai = openai_multi_body(
        &test_config("OpenAI Compatible", "https://example.com/v1"),
        &request,
        false,
    );
    let responses = openai_responses_multi_body(
        &test_config("OpenAI Compatible", "https://example.com/v1"),
        &request,
        false,
    );
    let anthropic = anthropic_multi_body(
        &test_config("Anthropic Messages", "https://example.com/v1"),
        &request,
        false,
    );
    assert_eq!(openai["parallel_tool_calls"], true);
    assert_eq!(responses["parallel_tool_calls"], true);
    assert!(anthropic.get("tool_choice").is_none());
}

#[test]
/// 公网 HTTP 和携带查询参数的地址会被拒绝。
fn rejects_unsafe_base_urls() {
    assert!(normalize_base_url("http://example.com/v1").is_err());
    assert!(normalize_base_url("https://example.com/v1?token=x").is_err());
    assert_eq!(
        normalize_base_url("http://localhost:11434/v1").unwrap(),
        "http://localhost:11434/v1/"
    );
}

#[test]
/// 工具缺失只允许首次请求后的三次额外重试。
fn limits_missing_tool_retries() {
    let error = CommandError::provider("PROVIDER_TOOL_NOT_CALLED", "没有工具调用");
    assert!(retry_missing_tool(0, &error, "trace"));
    assert!(retry_missing_tool(1, &error, "trace"));
    assert!(retry_missing_tool(2, &error, "trace"));
    assert!(!retry_missing_tool(3, &error, "trace"));
    let other = CommandError::provider("PROVIDER_REQUEST_FAILED", "请求失败");
    assert!(!retry_missing_tool(0, &other, "trace"));
}

#[test]
/// 工具缺失重试会强化提示词但不会发送 OpenAI tool_choice。
fn strengthens_retry_prompt_without_tool_choice() {
    let tool = test_tool();
    let request = ToolRequest {
        trace_id: "trace",
        turn: 1,
        system_prompt: "system",
        user_prompt: "user",
        images: &[],
        tool: &tool,
        max_tokens: 100,
        timeout: Duration::from_secs(5),
    };
    let body = openai_body(
        &test_config("OpenAI Compatible", "https://example.com/v1"),
        &request,
        true,
    );
    assert!(body["messages"][0]["content"]
        .as_str()
        .is_some_and(|value| value.contains("必须调用一个可用工具")));
    assert!(body.get("tool_choice").is_none());
}

#[tokio::test]
/// 单工具请求在首次无调用后会真实额外发起三次网络请求。
async fn retries_missing_tool_response_three_times() {
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("绑定测试端口失败");
    let address = listener.local_addr().expect("读取测试地址失败");
    let server = tokio::spawn(serve_missing_tool_stream(listener, 4));
    let client = AiClient::new().expect("创建客户端失败");
    let tool = test_tool();
    let config = test_config("OpenAI Compatible", &format!("http://{address}/v1"));
    let error = client
        .call_tool(
            &config,
            "secret",
            ToolRequest {
                trace_id: "retry-test",
                turn: 1,
                system_prompt: "必须调用工具",
                user_prompt: "调用 emit_result",
                images: &[],
                tool: &tool,
                max_tokens: 100,
                timeout: Duration::from_secs(5),
            },
        )
        .await
        .unwrap_err();
    assert_eq!(error.code, "PROVIDER_TOOL_NOT_CALLED");
    server.await.expect("测试服务任务失败");
}
