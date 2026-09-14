use super::*;
use crate::{
    services::{
        agent_tasks::{AgentControl, AgentTasks},
        subagents::{
            stored_children, ChildExecution, ChildExecutor, ChildFuture, SubagentLimits, Subagents,
        },
    },
    storage::testutil::TempDir,
};
use std::sync::Arc;

struct NoExecution;
impl ChildExecutor for NoExecution {
    /// 列表不得调用执行器；误启动必须直接让测试失败。
    fn execute(&self, _: ChildExecution, _: Arc<Subagents>, _: Arc<AgentControl>) -> ChildFuture {
        panic!("只读刷新不得启动模型");
    }
}

/// 创建真实父边与连续层级，扫描测试不绕过正常存储入口。
fn child(files: &AgentFiles, parent: &AgentSession) -> AgentSession {
    let session = AgentSession {
        id: Uuid::now_v7().to_string(),
        format_version: 1,
        parent_session_id: Some(parent.id.clone()),
        delegation_depth: parent.delegation_depth + 1,
        selected_paths: parent.selected_paths.clone(),
        ..Default::default()
    };
    ChildRepository::create(files, &session).unwrap();
    session
}

/// 线程本地计数只观察本测试真正执行的 read_dir，不受其他并行测试干扰。
fn scans() -> usize {
    CHILD_DIRECTORY_SCANS.with(|count| count.replace(0))
}

#[tokio::test]
/// 冷树、活动树和直接子刷新均只枚举一次目录，树越宽越深也不增加扫描次数。
async fn each_refresh_scans_directory_once() {
    let temp = TempDir::new();
    let files = Arc::new(AgentFiles::new(temp.path()).unwrap());
    let root = files.create_session().unwrap();
    for _ in 0..8 {
        let first = child(&files, &root);
        let second = child(&files, &first);
        child(&files, &second);
    }
    let other = files.create_session().unwrap();
    child(&files, &other);
    scans();
    let cold = stored_children(files.as_ref(), &root).unwrap();
    assert_eq!(cold.len(), 24);
    assert_eq!(scans(), 1);
    let (control, _) = AgentTasks::default()
        .register("window", "vault", "run", &root.id)
        .unwrap();
    let tree = Subagents::new(
        root.clone(),
        control,
        files.clone(),
        Arc::new(NoExecution),
        SubagentLimits::default(),
    );
    let active = tree.list(&root.id, true).unwrap();
    assert_eq!(scans(), 1);
    assert_eq!(
        serde_json::to_value(cold).unwrap(),
        serde_json::to_value(active).unwrap()
    );
    assert_eq!(tree.list(&root.id, false).unwrap().len(), 8);
    assert_eq!(scans(), 1);
    child(&files, &root);
    assert_eq!(tree.list(&root.id, true).unwrap().len(), 25);
    assert_eq!(scans(), 1);
    tree.shutdown().await;
    let closed = tree.observe_children(&root.id).unwrap();
    assert_eq!(closed.len(), 25);
    assert!(closed.iter().all(|child| child.status == "ready"));
    assert_eq!(scans(), 1);
}

#[test]
/// 文件名身份伪造与未来版本仍由原有安全读入口拒绝，不会变成目录 ready 项。
fn snapshot_rejects_mismatched_identity_and_future_version() {
    let temp = TempDir::new();
    let files = AgentFiles::new(temp.path()).unwrap();
    let root = files.create_session().unwrap();
    let mut saved = child(&files, &root);
    let path = files.record_path("sessions", &saved.id).unwrap();
    let original_id = saved.id.clone();
    saved.id = root.id.clone();
    envelope::save_json(&path, &saved).unwrap();
    assert!(stored_children(&files, &root).is_err());
    saved.id = original_id;
    saved.format_version = 999;
    envelope::save_json(&path, &saved).unwrap();
    assert!(stored_children(&files, &root).is_err());
}

#[test]
/// 目录仍拒绝错误层级，但父范围后来收窄不会隐藏历史孩子及其原写授权。
fn snapshot_revalidates_depth_but_preserves_historical_scopes() {
    let temp = TempDir::new();
    let files = AgentFiles::new(temp.path()).unwrap();
    let mut root = files.create_session().unwrap();
    root.selected_paths = vec!["a.md".into()];
    files.save_session(&root).unwrap();
    let parent = child(&files, &root);
    let valid = child(&files, &parent);
    let path = files.record_path("sessions", &valid.id).unwrap();
    let mut invalid = valid.clone();
    invalid.delegation_depth = 9;
    envelope::save_json(&path, &invalid).unwrap();
    assert!(stored_children(&files, &root).is_err());
    let mut historical = valid;
    historical.write_scope = vec!["a.md".into()];
    envelope::save_json(&path, &historical).unwrap();
    root.selected_paths = vec!["now.md".into()];
    root.write_scope = vec!["now.md".into()];
    files.save_session(&root).unwrap();
    scans();
    let listed = stored_children(&files, &root).unwrap();
    assert_eq!(listed.len(), 2);
    assert_eq!(listed[1].agent_id, historical.id);
    assert_eq!(listed[1].write_scope, historical.write_scope);
    assert_eq!(scans(), 1);
}
