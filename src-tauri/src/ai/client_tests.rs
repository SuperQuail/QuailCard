use super::super::ToolDefinition;
use super::*;
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
        models: Vec::new(),
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

/// 分段缓慢推送合法 SSE：单次事件间隔远小于空闲上限，但总时长超过它。
async fn serve_slow_text_stream(listener: TcpListener, response_count: usize) {
    let segments = [
        "data: {\"choices\":[{\"delta\":{\"content\":\"x\"}}]}\n\n",
        "data: {\"choices\":[{\"delta\":{\"content\":\"x\"}}]}\n\n",
        "data: {\"choices\":[{\"delta\":{\"content\":\"x\"}}]}\n\n",
        "data: [DONE]\n\n",
    ];
    let total: usize = segments.iter().map(|segment| segment.len()).sum();
    let header = format!(
        "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nContent-Length: {total}\r\nConnection: close\r\n\r\n"
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
            .write_all(header.as_bytes())
            .await
            .expect("写入响应头失败");
        for segment in segments {
            tokio::time::sleep(Duration::from_millis(150)).await;
            socket
                .write_all(segment.as_bytes())
                .await
                .expect("写入流式响应失败");
        }
    }
}

#[tokio::test]
/// 连续输出可以超过空闲上限的总时长；只有流真正空闲才判超时。
async fn continuous_stream_is_not_killed_by_total_time() {
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("绑定测试端口失败");
    let address = listener.local_addr().expect("读取测试地址失败");
    let server = tokio::spawn(serve_slow_text_stream(listener, 4));
    let client = ProviderGateway::new().expect("创建客户端失败");
    let tool = test_tool();
    let config = test_config("OpenAI Compatible", &format!("http://{address}/v1"));
    let error = client
        .call_tool(
            &config,
            "secret",
            ToolRequest {
                trace_id: "idle-test",
                turn: 1,
                system_prompt: "必须调用工具",
                user_prompt: "调用 emit_result",
                images: &[],
                tool: &tool,
                max_tokens: 100,
                timeout: Duration::from_millis(500),
            },
        )
        .await
        .unwrap_err();
    // 旧实现按整请求 500ms 上限会在这里返回 PROVIDER_TIMEOUT。
    assert_eq!(error.code, "PROVIDER_TOOL_NOT_CALLED");
    server.await.expect("测试服务任务失败");
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
    assert!(retry_missing_tool(0, &error));
    assert!(retry_missing_tool(1, &error));
    assert!(retry_missing_tool(2, &error));
    assert!(!retry_missing_tool(3, &error));
    let other = CommandError::provider("PROVIDER_REQUEST_FAILED", "请求失败");
    assert!(!retry_missing_tool(0, &other));
}

#[tokio::test]
/// 单工具请求在首次无调用后会真实额外发起三次网络请求。
async fn retries_missing_tool_response_three_times() {
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("绑定测试端口失败");
    let address = listener.local_addr().expect("读取测试地址失败");
    let server = tokio::spawn(serve_missing_tool_stream(listener, 4));
    let client = ProviderGateway::new().expect("创建客户端失败");
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
