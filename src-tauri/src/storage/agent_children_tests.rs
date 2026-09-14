use super::*;
use crate::storage::testutil::TempDir;

#[test]
/// 子身份独立创建且根列表不平铺子会话，父聚合中的实时内容不被覆盖。
fn children_do_not_overwrite_parent_and_stay_out_of_root_list() {
    let root = TempDir::new();
    let files = AgentFiles::new(root.path()).unwrap();
    let mut parent = files.create_session().unwrap();
    let child = AgentSession {
        id: Uuid::now_v7().to_string(),
        format_version: 1,
        parent_session_id: Some(parent.id.clone()),
        delegation_depth: 1,
        ..Default::default()
    };
    parent.title = "父任务最新状态".into();
    files.save_session(&parent).unwrap();
    ChildRepository::create(&files, &child).unwrap();
    assert_eq!(files.sessions().unwrap().len(), 1);
    assert_eq!(files.session(&parent.id).unwrap().title, parent.title);
    let children = ChildRepository::list(&files, &parent.id).unwrap();
    assert_eq!(children.len(), 1);
    assert_eq!(children[0].id, child.id);
    assert!(ChildRepository::create(&files, &child).is_err());
}

#[test]
/// 不存在的父身份、伪造派生深度与路径穿越全部在存储入口拒绝。
fn rejects_invalid_child_identity() {
    let root = TempDir::new();
    let files = AgentFiles::new(root.path()).unwrap();
    let parent = files.create_session().unwrap();
    let mut child = AgentSession {
        id: Uuid::now_v7().to_string(),
        format_version: 1,
        parent_session_id: Some(parent.id),
        delegation_depth: 3,
        ..Default::default()
    };
    assert!(ChildRepository::create(&files, &child).is_err());
    child.delegation_depth = 1;
    child.id = "../other".into();
    assert!(ChildRepository::create(&files, &child).is_err());
    child.id = Uuid::now_v7().to_string();
    child.parent_session_id = Some(Uuid::now_v7().to_string());
    assert!(ChildRepository::create(&files, &child).is_err());
}

#[test]
/// Goal 与计划恢复不会包含 armed 许可；新字段缺失仍兼容旧会话。
fn autonomy_roundtrips_without_runtime_permission() {
    let root = TempDir::new();
    let files = AgentFiles::new(root.path()).unwrap();
    let mut session = files.create_session().unwrap();
    session.goal = Some(crate::agent_autonomy_models::Goal {
        id: Uuid::now_v7().to_string(),
        revision: 4,
        objective: "完成笔记分析".into(),
        acceptance_criteria: vec!["提供依据".into()],
        phase: crate::agent_autonomy_models::GoalPhase::Active,
        rounds_started: 2,
        max_goal_rounds: 16,
        ..Default::default()
    });
    session.plan.owner_session_id = session.id.clone();
    session.plan.revision = 3;
    files.save_session(&session).unwrap();
    let restored = files.session(&session.id).unwrap();
    assert_eq!(restored.goal, session.goal);
    assert_eq!(restored.plan, session.plan);
    assert!(serde_json::to_value(&restored)
        .unwrap()
        .get("armed")
        .is_none());
    let legacy: AgentSession =
        serde_json::from_value(json!({"id":session.id,"formatVersion":1})).unwrap();
    assert!(legacy.goal.is_none());
    assert_eq!(legacy.completed_message_count, 0);
}
