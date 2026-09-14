//! 请求诊断必须覆盖空内容、超时和取消，且不能泄露任何模型材料。
use super::*;
use crate::ai::ToolDefinition;
use crate::services::agent_ports::{AgentFuture, AgentModelReply};
use serde_json::Value;
use std::sync::Mutex;

struct Fake(&'static str);
impl AgentModel for Fake {
    /// 用固定分支模拟供应商结果，不访问网络或写入用户数据。
    fn call<'a>(
        &'a self,
        _: &'a str,
        _: &'a [Value],
        _: &'a [ToolDefinition],
        delta: &'a (dyn Fn(&str) + Send + Sync),
        _reasoning: &'a (dyn Fn(&str) + Send + Sync),
    ) -> AgentFuture<'a, AgentModelReply> {
        Box::pin(async move {
            match self.0 {
                "timeout" => Err(CommandError::new("PROVIDER_TIMEOUT", "SECRET_ERROR_TOKEN")),
                "pending" => std::future::pending().await,
                "empty" => {
                    delta("SECRET_STREAM");
                    Ok(AgentModelReply::default())
                }
                _ => Ok(AgentModelReply {
                    text: "SECRET_NOTE".into(),
                    ..Default::default()
                }),
            }
        })
    }
}

#[tokio::test]
/// 完成和失败只记录计数与安全分类，提示词、流片段、回复及错误原文均不可见。
async fn logs_empty_timeout_success_without_content() {
    for (kind, expected) in [
        ("empty", "outcome=empty"),
        ("timeout", "code=PROVIDER_TIMEOUT"),
        ("ok", "outcome=completed"),
    ] {
        let lines = Mutex::new(Vec::new());
        let log = |line: &str| lines.lock().unwrap().push(line.to_string());
        let result = call(&Fake(kind), "SECRET_PROMPT", "最终成稿", &log).await;
        assert_eq!(result.is_ok(), kind == "ok");
        let lines = lines.into_inner().unwrap();
        assert_eq!(lines.len(), 2);
        let text = lines.join(
            "
",
        );
        assert!(text.contains(expected));
        assert!(text.contains("prompt_chars=13"));
        assert!(text.contains("elapsed_ms="));
        assert!(!text.contains("SECRET"));
        let trace = lines[0]
            .split("trace=")
            .nth(1)
            .unwrap()
            .split_whitespace()
            .next()
            .unwrap();
        assert!(uuid::Uuid::parse_str(trace).is_ok());
        assert!(lines[1].contains(trace));
    }
}

#[tokio::test]
/// 取消悬挂请求会记录中断终态，而不是留下仅有开始的日志。
async fn dropped_request_records_interruption() {
    let lines = Mutex::new(Vec::new());
    let log = |line: &str| lines.lock().unwrap().push(line.to_string());
    assert!(tokio::time::timeout(
        std::time::Duration::from_millis(10),
        call(&Fake("pending"), "SECRET_PROMPT", "分块压缩 1/2", &log)
    )
    .await
    .is_err());
    let lines = lines.into_inner().unwrap();
    assert_eq!(lines.len(), 2);
    assert!(lines[1].contains("outcome=interrupted"));
    assert_eq!(safe_code("SECRET_CODE"), "OTHER_ERROR");
}
