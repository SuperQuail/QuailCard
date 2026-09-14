use super::*;
use crate::{
    services::{agent_ports::AgentRepository, subagents::ChildRepository},
    storage::testutil::TempDir,
};

/// 真实存储创建子身份，不绕过父关系和深度规则。
fn child(files: &AgentFiles, parent: &AgentSession) -> AgentSession {
    let child = AgentSession {
        format_version: 1,
        id: uuid::Uuid::now_v7().to_string(),
        parent_session_id: Some(parent.id.clone()),
        delegation_depth: parent.delegation_depth + 1,
        ..Default::default()
    };
    ChildRepository::create(files, &child).unwrap();
    child
}

#[test]
/// 有效任务跨窗口、跨 Vault 或跨根会话都拒绝，不能被当成缺失历史降级。
fn valid_runs_require_window_vault_and_session() {
    let tasks = AgentTasks::default();
    tasks.register("main", "vault-a", "run", "root").unwrap();
    assert!(control(&tasks, "main", Path::new("vault-a"), "root", "run")
        .unwrap()
        .is_some());
    assert!(control(&tasks, "other", Path::new("vault-a"), "root", "run").is_err());
    assert!(control(&tasks, "main", Path::new("vault-b"), "root", "run").is_err());
    assert!(control(&tasks, "main", Path::new("vault-a"), "other", "run").is_err());
    assert!(
        control(&tasks, "main", Path::new("vault-a"), "root", "missing")
            .unwrap()
            .is_none()
    );
}

#[test]
/// 授权热路径不重读祖先历史；子身份、根自身、深度跳跃与伪父链均复验。
fn lineage_permissions_use_only_cached_metadata_on_repeat() {
    let temp = TempDir::new();
    let files = AgentFiles::new(temp.path()).unwrap();
    let root = files.create_session().unwrap();
    let other = files.create_session().unwrap();
    let direct = child(&files, &root);
    let mut grandchild = child(&files, &direct);
    let count = AgentFiles::session_read_count();
    authorize_history(&files, &root.id, Some(&grandchild.id)).unwrap();
    authorize_history(&files, &root.id, Some(&grandchild.id)).unwrap();
    assert_eq!(AgentFiles::session_read_count(), count);
    assert!(authorize_history(&files, &other.id, Some(&grandchild.id)).is_err());
    assert!(authorize_history(&files, &root.id, Some(&root.id)).is_err());
    assert!(authorize_history(&files, &direct.id, None).is_err());
    grandchild.delegation_depth = 99;
    files.save_session(&grandchild).unwrap();
    assert!(authorize_history(&files, &root.id, Some(&grandchild.id)).is_err());
    grandchild.parent_session_id = Some(grandchild.id.clone());
    files.save_session(&grandchild).unwrap();
    assert!(authorize_history(&files, &root.id, Some(&grandchild.id)).is_err());
}

#[test]
/// 即便历史版本未变，运行状态仍每次返回；停止/关闭查询不重新启动工作。
fn runtime_sampling_is_independent_from_history_revision() {
    let tasks = AgentTasks::default();
    let (run, _) = tasks.register("main", "vault", "run", "root").unwrap();
    let first = sample_run(Some(&run), "root", None, true).unwrap().unwrap();
    run.update(|state| state.text = "新增文字".into());
    let next = sample_run(Some(&run), "root", None, true).unwrap().unwrap();
    assert_eq!(next.id, "run");
    assert!(next.sequence > first.sequence);
    run.cancel();
    run.complete(None);
    assert_eq!(
        sample_run(Some(&run), "root", None, true)
            .unwrap()
            .unwrap()
            .state,
        "cancelled"
    );
    assert!(sample_run(Some(&run), "root", Some("child"), true)
        .unwrap()
        .is_none());
    assert!(sample_run(None, "root", None, true).is_err());
    assert!(sample_run(None, "root", Some("child"), true)
        .unwrap()
        .is_none());
    assert!(sample_run(None, "root", None, false).unwrap().is_none());
}

#[test]
/// 先采样运行后读取历史，即使提交事件清空流文本也至少保留一个正文来源。
fn committed_text_remains_in_history_after_runtime_sampling() {
    use crate::services::agent_events::{AgentEvent, AgentEventSink};
    let temp = TempDir::new();
    let files = AgentFiles::new(temp.path()).unwrap();
    let mut session = files.create_session().unwrap();
    let tasks = AgentTasks::default();
    let (control, _) = tasks.register("main", "vault", "run", &session.id).unwrap();
    let (revision, _) = files.observe_history(&session.id, None).unwrap();
    control.update(|state| state.text = "已保存正文".into());
    let sampled = sample_run(Some(&control), &session.id, None, true).unwrap();
    session.messages.push(crate::agent_models::AgentMessage {
        id: "message".into(),
        kind: "text".into(),
        content: "已保存正文".into(),
        ..Default::default()
    });
    files.save_session(&session).unwrap();
    control.emit(AgentEvent::TextCommitted {
        message_id: "message".into(),
    });
    let (next, history) = files.observe_history(&session.id, Some(&revision)).unwrap();
    assert_ne!(revision, next);
    assert!(!sampled.unwrap().text.is_empty());
    assert_eq!(history.unwrap().messages[0].content, "已保存正文");
}

#[test]
/// wire 显式保留 null 字段，实际目标身份不被父会话替代。
fn observation_wire_keeps_nullable_fields() {
    let wire = serde_json::to_value(AgentObservation {
        session_id: "child".into(),
        revision: "v1:token".into(),
        session: None,
        run: None,
        writes: vec![],
    })
    .unwrap();
    assert_eq!(wire["sessionId"], "child");
    assert!(wire.get("session").unwrap().is_null());
    assert!(wire.get("run").unwrap().is_null());
    assert!(wire["writes"].as_array().unwrap().is_empty());
    assert_eq!(wire.as_object().unwrap().len(), 5);
}
