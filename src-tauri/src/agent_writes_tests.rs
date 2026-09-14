use super::*;
use crate::services::agent_tasks::AgentTasks;

/// 根执行尚未挂载执行树时，保存等待仍可被查询并正常确认。
#[tokio::test]
async fn root_write_without_a_tree_is_queryable_and_confirmable() {
    let (control, _) = AgentTasks::default()
        .register("main", "vault", "run", "root")
        .unwrap();
    assert!(pending(&control).is_empty());
    let waiting = {
        let control = control.clone();
        tokio::spawn(async move { control.prepare_write("note.md", "operation").await })
    };
    while control.pending_write().is_none() {
        tokio::task::yield_now().await;
    }
    let writes = pending(&control);
    assert_eq!(writes.len(), 1);
    assert_eq!(writes[0].execution_id, "run");
    assert_eq!(writes[0].session_id, "root");
    assert_eq!(writes[0].path, "note.md");
    assert_eq!(writes[0].operation_id, "operation");
    let acknowledging = {
        let control = control.clone();
        tokio::spawn(async move { acknowledge(&control, "run", "operation").await })
    };
    assert!(waiting.await.unwrap().is_ok());
    // 工具真正写入后才清除标记，确认据此收尾。
    control.update(|state| {
        state.pending_write = None;
        state.pending_write_id = None;
    });
    assert!(acknowledging.await.unwrap().is_ok());
    assert!(pending(&control).is_empty());
}

/// wire 只暴露协调必需身份，不携带工具参数、正文或磁盘绝对路径。
#[test]
fn write_wire_uses_camel_case_and_keeps_identity_only() {
    let wire = serde_json::to_value(AgentPendingWrite::from(PendingWrite {
        execution_id: "run".into(),
        session_id: "child".into(),
        path: "note.md".into(),
        operation: "operation".into(),
    }))
    .unwrap();
    assert_eq!(wire["executionId"], "run");
    assert_eq!(wire["sessionId"], "child");
    assert_eq!(wire["path"], "note.md");
    assert_eq!(wire["operationId"], "operation");
    assert_eq!(wire.as_object().unwrap().len(), 4);
}

/// 外来执行身份与过期操作都不能借用根权限推进保存。
#[tokio::test]
async fn foreign_execution_and_stale_operation_are_rejected() {
    let (control, _) = AgentTasks::default()
        .register("main", "vault", "run", "root")
        .unwrap();
    let waiting = {
        let control = control.clone();
        tokio::spawn(async move { control.prepare_write("note.md", "operation").await })
    };
    while control.pending_write().is_none() {
        tokio::task::yield_now().await;
    }
    assert!(acknowledge(&control, "other", "operation").await.is_err());
    assert!(acknowledge(&control, "run", "stale").await.is_err());
    assert_eq!(pending(&control).len(), 1);
    control.cancel();
    assert!(waiting.await.unwrap().is_err());
}
