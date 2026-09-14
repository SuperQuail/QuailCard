//! 前端历史的纯投影边界：磁盘 exchange 保持原样，公开消息只含白名单摘要。
use crate::agent_models::{
    AgentMessage, AgentSession, AgentToolCalls, AgentToolRow, AgentToolState,
};
use serde::Deserialize;
use serde_json::Value;
use sha2::{Digest, Sha256};

#[path = "agent_tool_details.rs"]
mod details;
pub(crate) use details::safe_arguments;

/// 消费读取副本而不触碰存储；一对一替换保证消息顺序与完成边界不变。
pub(crate) fn project(mut session: AgentSession) -> AgentSession {
    let names = crate::services::agent::registered_tool_names();
    for (index, message) in session.messages.iter_mut().enumerate() {
        if message.kind == "exchange" {
            *message = exchange(message, index, &names);
        }
    }
    session
}

/// 只读取中立 calls/results；旧记录不解析 assistant 或任何供应商续传字段。
fn exchange(message: &AgentMessage, index: usize, names: &[&str]) -> AgentMessage {
    let identity = format!(
        "tool-history-{:x}-{index}",
        Sha256::digest(message.id.as_bytes())
    );
    let results = message.data["results"]
        .as_array()
        .map(Vec::as_slice)
        .unwrap_or(&[]);
    let calls = message.data["calls"]
        .as_array()
        .map(Vec::as_slice)
        .unwrap_or(&[]);
    let mut rows = Vec::new();
    if calls.is_empty() {
        // 没有中立调用名时只展示结果状态，绝不从原始协议猜测工具名称。
        for (row_index, result) in results.iter().enumerate() {
            rows.push(row(
                &identity,
                row_index,
                "unknown_tool",
                result_state(Some(result)),
            ));
        }
        if rows.is_empty() {
            rows.push(row(&identity, 0, "unknown_tool", AgentToolState::Error));
        }
    } else {
        for (row_index, call) in calls.iter().enumerate() {
            let name = call["name"]
                .as_str()
                .and_then(|name| names.iter().copied().find(|registered| *registered == name))
                .unwrap_or("unknown_tool");
            let matched = call_result(call, calls, results);
            let state = matched.map(result_state).unwrap_or(AgentToolState::Error);
            let mut projected = row(&identity, row_index, name, state);
            details::enrich(&mut projected, &call["arguments"], matched.ok().flatten());
            rows.push(projected);
        }
    }
    AgentMessage {
        id: identity,
        role: "assistant".into(),
        content: "工具记录".into(),
        kind: "tool_calls".into(),
        data: serde_json::json!(AgentToolCalls { rows }),
    }
}

/// 本地身份和序号构成稳定公开 ID，状态摘要仅来自静态文案。
fn row(identity: &str, index: usize, name: &str, state: AgentToolState) -> AgentToolRow {
    let summary = match state {
        AgentToolState::Running => "工具尚未完成",
        AgentToolState::Ok => "工具执行完成",
        AgentToolState::Error => "工具记录不可用，无法确认执行结果",
    };
    AgentToolRow {
        id: format!("{identity}-{index}"),
        name: name.into(),
        state,
        summary: summary.into(),
        ..Default::default()
    }
}

/// 供应商 ID 仅用于内存中的精确配对，重复或缺失身份不能冒充成功收据。
fn call_result<'a>(
    call: &Value,
    calls: &[Value],
    results: &'a [Value],
) -> Result<Option<&'a Value>, ()> {
    let Some(id) = call["id"].as_str().filter(|id| !id.is_empty()) else {
        return Err(());
    };
    if calls
        .iter()
        .filter(|call| call["id"].as_str() == Some(id))
        .count()
        != 1
    {
        return Err(());
    }
    let mut matched = results
        .iter()
        .filter(|result| result["tool_call_id"].as_str() == Some(id));
    let result = matched.next();
    if matched.next().is_some() {
        return Err(());
    }
    Ok(result)
}

/// 只反序列化状态字段，未知载荷被忽略，避免复制参数、结果正文或凭据。
#[derive(Default, Deserialize)]
#[serde(default)]
struct Receipt {
    ok: Option<bool>,
    status: Option<ReceiptStatus>,
}

/// 已保存的中立执行占位符，不依赖供应商协议名称。
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
enum ReceiptStatus {
    NotExecuted,
    #[serde(other)]
    Other,
}

/// 缺结果与尚未执行显示未完成；不可解析的既有结果保守显示错误而非成功。
fn result_state(result: Option<&Value>) -> AgentToolState {
    let Some(result) = result else {
        return AgentToolState::Running;
    };
    let receipt = details::receipt_text(result)
        .and_then(|content| serde_json::from_str::<Receipt>(content).ok());
    match receipt {
        Some(Receipt {
            ok: Some(false),
            status: Some(ReceiptStatus::NotExecuted),
        }) => AgentToolState::Running,
        Some(Receipt {
            ok: Some(true),
            status: None,
        }) => AgentToolState::Ok,
        _ => AgentToolState::Error,
    }
}

#[cfg(test)]
#[path = "agent_tool_details_tests.rs"]
mod detail_tests;

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    /// 测试夹具只提供中立记录，额外敏感字段用于验证投影不穿透。
    fn session(data: Value) -> AgentSession {
        AgentSession {
            id: "local-session".into(),
            completed_message_count: 1,
            messages: vec![AgentMessage {
                id: "local-message".into(),
                role: "assistant".into(),
                kind: "exchange".into(),
                content: "SECRET-content".into(),
                data,
            }],
            ..Default::default()
        }
    }

    /// 结果顺序不必等于调用顺序；缺结果与执行占位符都不能被误报成功。
    #[test]
    fn matches_neutral_receipts_and_keeps_message_boundaries() {
        let original = session(json!({
            "calls": [{"id":"a","name":"read_note"},{"id":"b","name":"search_notes"},
                {"id":"c","name":"read_note"},{"id":"d","name":"read_note"}],
            "results": [
                {"tool_call_id":"b","content":"{\"ok\":false,\"error\":\"SECRET-error\"}"},
                {"tool_call_id":"c","content":"{\"ok\":false,\"status\":\"notExecuted\"}"},
                {"tool_call_id":"a","content":"{\"ok\":true,\"result\":\"SECRET-output\"}"}
            ]
        }));
        let original_json = serde_json::to_value(&original).unwrap();
        let public = project(original.clone());
        let rows = &public.messages[0].data["rows"];
        assert_eq!(rows[0]["name"], "read_note");
        assert_eq!(rows[0]["state"], "ok");
        assert_eq!(rows[1]["state"], "error");
        assert_eq!(rows[2]["state"], "running");
        assert_eq!(rows[3]["state"], "running");
        assert_eq!(public.messages.len(), 1);
        assert_eq!(public.completed_message_count, 1);
        assert_eq!(serde_json::to_value(&original).unwrap(), original_json);
        assert_eq!(original.messages[0].kind, "exchange");
        assert_eq!(public.messages[0].kind, "tool_calls");
        assert!(!serde_json::to_string(&public).unwrap().contains("SECRET"));
    }

    /// 名称、调用身份、参数、输出及协议字段的注入不能进入任意公开字段。
    #[test]
    fn rejects_injected_names_ids_and_all_raw_payloads() {
        let mut original = session(json!({
            "calls": [{"id":"SECRET-id","name":"read_note SECRET-name", "arguments":"SECRET-args"}],
            "results": [{"tool_call_id":"SECRET-id","content":"{\"ok\":true,\"token\":\"SECRET-token\"}"}],
            "assistant": {"protocol":"SECRET-protocol","apiKey":"SECRET-key"},
            "credential":"SECRET-credential"
        }));
        original.messages[0].id = "SECRET-message-id".into();
        original.messages[0].role = "SECRET-role".into();
        let public = project(original.clone());
        let serialized = serde_json::to_string(&public).unwrap();
        assert!(!serialized.contains("SECRET"));
        let data = &public.messages[0].data;
        assert_eq!(data.as_object().unwrap().len(), 1);
        assert_eq!(data["rows"][0].as_object().unwrap().len(), 4);
        assert_eq!(data["rows"][0]["name"], "unknown_tool");
        assert_eq!(data["rows"][0]["summary"], "工具执行完成");
        assert_eq!(
            serde_json::to_value(project(original)).unwrap(),
            serde_json::to_value(&public).unwrap()
        );
        assert_eq!(
            serde_json::to_value(project(public.clone())).unwrap(),
            serde_json::to_value(public).unwrap()
        );
    }

    /// 旧记录只依赖中立结果；任意未知协议数据与畸形记录都安全降级。
    #[test]
    fn legacy_and_malformed_exchanges_never_parse_protocol_payloads() {
        for assistant in [
            json!({"arbitrary":"SECRET"}),
            json!(["SECRET"]),
            Value::Null,
        ] {
            let public = project(session(json!({"assistant":assistant,"results":[
                {"name":"read_note", "content":"{\"ok\":true}"},
                {"content":"SECRET-invalid-json"}
            ]})));
            let rows = &public.messages[0].data["rows"];
            assert_eq!(rows[0]["name"], "unknown_tool");
            assert_eq!(rows[0]["state"], "ok");
            assert_eq!(rows[1]["state"], "error");
            assert!(!serde_json::to_string(&public).unwrap().contains("SECRET"));
        }
        for data in [
            Value::Null,
            json!({}),
            json!({"calls":[],"results":"SECRET"}),
        ] {
            let public = project(session(data));
            assert_eq!(public.messages[0].data["rows"][0]["state"], "error");
        }
    }

    /// 模型重复调用 ID 或重复结果不能交叉认领成功，公开行 ID 仍唯一。
    #[test]
    fn duplicate_or_missing_ids_fail_closed() {
        for calls in [json!([{"id":"x"},{"id":"x"}]), json!([{}, {}])] {
            let public = project(session(json!({"calls":calls,"results":[
                {"tool_call_id":"x","content":"{\"ok\":true}"}
            ]})));
            let rows = &public.messages[0].data["rows"];
            assert_eq!(rows[0]["state"], "error");
            assert_eq!(rows[1]["state"], "error");
            assert_ne!(rows[0]["id"], rows[1]["id"]);
        }
        let public = project(session(json!({"calls":[{"id":"x"}],"results":[
            {"tool_call_id":"x","content":"{\"ok\":true}"},
            {"tool_call_id":"x","content":"{\"ok\":true}"}
        ]})));
        assert_eq!(public.messages[0].data["rows"][0]["state"], "error");
    }

    /// 三类注册表的真实名称均可显示，大小写、前后空白或控制字符不能绕过精确匹配。
    #[test]
    fn registry_names_cover_business_runtime_and_generation_tools() {
        let names = crate::services::agent::registered_tool_names();
        for expected in [
            "read_note",
            "get_goal",
            "get_plan",
            "emit_card",
            "lookup_words",
        ] {
            assert!(names.contains(&expected));
        }
        for name in names {
            let public = project(session(json!({"calls":[{"id":"a","name":name}]})));
            assert_eq!(public.messages[0].data["rows"][0]["name"], name);
        }
        for name in ["READ_NOTE", " read_note", "read_note\n", "read_note\0"] {
            let public = project(session(json!({"calls":[{"id":"a","name":name}]})));
            assert_eq!(public.messages[0].data["rows"][0]["name"], "unknown_tool");
        }
    }

    /// 公开身份不受供应商调用 ID 变化影响，相同本地身份的多个消息也不碰撞。
    #[test]
    fn public_ids_depend_only_on_local_message_identity_and_indices() {
        let first = session(json!({"calls":[{"id":"SECRET-a","name":"read_note"}]}));
        let mut second = first.clone();
        second.messages[0].data["calls"][0]["id"] = json!("SECRET-b");
        assert_eq!(
            project(first).messages[0].data,
            project(second.clone()).messages[0].data
        );
        second.messages.push(second.messages[0].clone());
        let public = project(second);
        assert_ne!(public.messages[0].id, public.messages[1].id);
        assert_ne!(
            public.messages[0].data["rows"][0]["id"],
            public.messages[1].data["rows"][0]["id"]
        );
    }

    /// DTO 增量字段保持容忍读取，非 exchange 消息原样保留。
    #[test]
    fn defaults_unknown_fields_and_visible_messages_remain_compatible() {
        let dto: AgentToolCalls = serde_json::from_value(json!({"future":true})).unwrap();
        assert!(dto.rows.is_empty());
        let row: AgentToolRow =
            serde_json::from_value(json!({"state":"future","extra":true})).unwrap();
        assert_eq!(row.state, AgentToolState::Error);
        let mut original = session(Value::Null);
        original.messages[0].kind = "text".into();
        original.messages[0].content = "原有可见正文".into();
        assert_eq!(
            serde_json::to_value(project(original.clone())).unwrap(),
            serde_json::to_value(original).unwrap()
        );
    }
}
