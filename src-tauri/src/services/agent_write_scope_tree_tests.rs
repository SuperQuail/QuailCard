// 子代理写入范围：执行树（派生/冷恢复/状态输出）与按路径并发锁用例。
// （本文件被 include! 进 tests 模块，不能用 //! 内层文档注释。）
use super::write_scope::{call, hash, results, ScopeModel, DICTIONARY, LEARNING, VIDEO};
use super::*;
use crate::{
    services::{
        agent_tasks::{AgentControl, AgentTasks},
        subagents::{
            ChildExecution, ChildExecutor, ChildFuture, ChildRepository, SubagentLimits, Subagents,
        },
    },
    storage::agent::AgentFiles,
};
use std::{
    collections::BTreeMap,
    sync::{Arc, Mutex},
    time::Duration,
};

/// 内存仓库：只保存子会话身份，测试不接触真实文件系统。
#[derive(Default)]
struct FakeRepo(Mutex<BTreeMap<String, AgentSession>>);
impl ChildRepository for FakeRepo {
    /// 创建拒绝覆盖已有身份，与真实仓库契约一致。
    fn create(&self, child: &AgentSession) -> Result<(), CommandError> {
        let mut rows = self.0.lock().unwrap();
        if rows.insert(child.id.clone(), child.clone()).is_some() {
            return Err(CommandError::validation("子会话身份已存在"));
        }
        Ok(())
    }
    /// 读取最新子会话；管理器随后复验父关系与范围。
    fn load(&self, id: &str) -> Result<AgentSession, CommandError> {
        self.0
            .lock()
            .unwrap()
            .get(id)
            .cloned()
            .ok_or_else(|| CommandError::new("SUBAGENT_FORBIDDEN", "无权操作该子 Agent"))
    }
    /// 按持久父关系枚举子会话。
    fn list(&self, parent_id: &str) -> Result<Vec<AgentSession>, CommandError> {
        Ok(self
            .0
            .lock()
            .unwrap()
            .values()
            .filter(|session| session.parent_session_id.as_deref() == Some(parent_id))
            .cloned()
            .collect())
    }
}

/// 即时回执的执行器：授权校验在派生阶段完成，本组测试不驱动子模型。
struct DoneExecutor;
impl ChildExecutor for DoneExecutor {
    /// 子轮次立即完成，宿主不必等待模型。
    fn execute(&self, _: ChildExecution, _: Arc<Subagents>, _: Arc<AgentControl>) -> ChildFuture {
        Box::pin(async { Ok("完成".into()) })
    }
}

/// 用内存仓库装配执行树；根身份固定为 root。
fn tree_with_repo(repo: Arc<FakeRepo>) -> Arc<Subagents> {
    let root = AgentSession {
        id: "root".into(),
        ..Default::default()
    };
    let (control, _) = AgentTasks::default()
        .register("window", "vault", "run", &root.id)
        .unwrap();
    Subagents::new(
        root,
        control,
        repo,
        Arc::new(DoneExecutor),
        SubagentLimits::default(),
    )
}

/// 直接子的身份快照；管理器只信树内节点，不信这个入参。
fn child_identity(id: &str) -> AgentSession {
    AgentSession {
        id: id.into(),
        parent_session_id: Some("root".into()),
        delegation_depth: 1,
        ..Default::default()
    }
}

#[tokio::test]
/// 孙代理不能拿到父范围之外的路径：只能收窄，整库与越权申请都被拒绝。
async fn grandchild_cannot_escape_parent_write_scope() {
    let repo = Arc::new(FakeRepo::default());
    let manager = tree_with_repo(repo.clone());
    let root = AgentSession {
        id: "root".into(),
        ..Default::default()
    };
    let child = manager
        .spawn_granted(
            "root",
            &root,
            "任务",
            "child",
            false,
            vec![],
            vec!["notes/".into()],
        )
        .await
        .unwrap();
    assert_eq!(
        repo.load(&child).unwrap().write_scope,
        vec!["notes/".to_string()]
    );
    let listed = manager.list("root", false).unwrap();
    assert_eq!(listed[0].write_scope, vec!["notes/".to_string()]);
    assert_eq!(
        serde_json::to_value(&listed[0]).unwrap()["writeScope"],
        json!(["notes/"])
    );
    let parent = child_identity(&child);
    let grandchild = manager
        .spawn_granted(
            &child,
            &parent,
            "孙",
            "g",
            false,
            vec![],
            vec!["notes/sub.md".into()],
        )
        .await
        .unwrap();
    assert_eq!(
        repo.load(&grandchild).unwrap().write_scope,
        vec!["notes/sub.md".to_string()]
    );
    for denied in [
        vec!["other/".to_string()],
        vec!["/".to_string()],
        vec!["notes/a.md".to_string(), "other.md".to_string()],
        vec!["../escape.md".to_string()],
    ] {
        assert!(
            manager
                .spawn_granted(&child, &parent, "孙", "g", false, vec![], denied.clone())
                .await
                .is_err(),
            "{denied:?} 必须被拒"
        );
    }
    // 只读子代理自己不能再授予任何写权限。
    let read_only = manager
        .spawn("root", &root, "任务", "read-only", false, vec![])
        .await
        .unwrap();
    assert!(manager
        .spawn_granted(
            &read_only,
            &child_identity(&read_only),
            "孙",
            "g",
            false,
            vec![],
            vec!["notes/".into()],
        )
        .await
        .is_err());
    manager.shutdown().await;
}

#[tokio::test]
/// 冷恢复不能被磁盘旧文件放大写权：越权记录被拒，范围内的记录保留授权。
async fn cold_restore_rechecks_write_scope() {
    let repo = Arc::new(FakeRepo::default());
    let manager = tree_with_repo(repo.clone());
    let root = AgentSession {
        id: "root".into(),
        ..Default::default()
    };
    let child = manager
        .spawn_granted(
            "root",
            &root,
            "任务",
            "child",
            false,
            vec![],
            vec!["notes/".into()],
        )
        .await
        .unwrap();
    for (id, scope) in [
        ("narrow", vec!["notes/sub.md".to_string()]),
        ("wide", vec!["other/".to_string()]),
    ] {
        repo.create(&AgentSession {
            id: id.into(),
            parent_session_id: Some(child.clone()),
            delegation_depth: 2,
            write_scope: scope,
            ..Default::default()
        })
        .unwrap();
    }
    assert!(manager.send(&child, "wide", "恢复").await.is_err());
    manager.send(&child, "narrow", "恢复").await.unwrap();
    let restored = manager
        .list(&child, false)
        .unwrap()
        .into_iter()
        .find(|info| info.agent_id == "narrow")
        .expect("范围内的记录必须可冷恢复");
    assert_eq!(restored.write_scope, vec!["notes/sub.md".to_string()]);
    manager.shutdown().await;
}

#[test]
/// 并发写：不同路径互不阻塞，同一路径串行，陈旧 hash 仍然被拒。
fn path_locks_parallelize_distinct_notes_and_serialize_same_note() {
    let root = TempDir::new();
    let files = Arc::new(AgentFiles::new(root.path()).unwrap());
    files
        .change(&uuid::Uuid::now_v7().to_string(), "a.md", "A1", None)
        .unwrap();
    files
        .change(&uuid::Uuid::now_v7().to_string(), "b.md", "B1", None)
        .unwrap();
    // 1) 持有 a.md 的路径锁时，b.md 仍能写入：不存在串行化全部笔记的全局锁。
    let held = crate::storage::agent::lock_note_path("a.md").unwrap();
    let guard = held.lock().unwrap();
    let (sender, receiver) = std::sync::mpsc::channel();
    let worker = {
        let files = files.clone();
        std::thread::spawn(move || {
            let result = files.change(
                &uuid::Uuid::now_v7().to_string(),
                "b.md",
                "B2",
                Some(&hash("B1")),
            );
            let _ = sender.send(result.is_ok());
        })
    };
    assert!(
        receiver.recv_timeout(Duration::from_secs(5)).is_ok(),
        "不同路径不得互相阻塞"
    );
    worker.join().unwrap();
    drop(guard);
    // 2) 持有 a.md 的路径锁时，同一路径的写入必须等待，释放后才完成。
    let held = crate::storage::agent::lock_note_path("a.md").unwrap();
    let guard = held.lock().unwrap();
    let (sender, receiver) = std::sync::mpsc::channel();
    let worker = {
        let files = files.clone();
        std::thread::spawn(move || {
            let result = files.change(
                &uuid::Uuid::now_v7().to_string(),
                "a.md",
                "A2",
                Some(&hash("A1")),
            );
            let _ = sender.send(result.is_ok());
        })
    };
    assert!(
        receiver.recv_timeout(Duration::from_millis(200)).is_err(),
        "同一路径必须串行"
    );
    drop(guard);
    assert!(receiver.recv_timeout(Duration::from_secs(5)).unwrap());
    worker.join().unwrap();
    assert_eq!(files.read("a.md").unwrap()["content"], "A2");
    // 3) 陈旧 hash 仍是冲突错误，子代理据此重读重写。
    let stale = files
        .change(
            &uuid::Uuid::now_v7().to_string(),
            "a.md",
            "A3",
            Some(&hash("A1")),
        )
        .err()
        .unwrap();
    assert_eq!(stale.code, "AGENT_NOTE_CONFLICT");
    assert_eq!(files.read("a.md").unwrap()["content"], "A2");
}

#[tokio::test]
/// 根会话把目标目录授权给子代理并让它自己落盘；状态输出带出 writeScope。
async fn root_grants_child_scope_and_status_reports_it() {
    let root = TempDir::new();
    std::fs::create_dir_all(root.path().join("notes")).unwrap();
    let files = Arc::new(AgentFiles::new(root.path()).unwrap());
    let mut session = files.create_session().unwrap();
    session.title = "root".into();
    files.save_session(&session).unwrap();
    let input = AgentInput {
        session_id: session.id.clone(),
        request_id: uuid::Uuid::now_v7().to_string(),
        content: "整理笔记".into(),
        provider_id: "test".into(),
        selected_paths: vec![],
        images: vec![],
    };
    let (control, _) = AgentTasks::default()
        .register("main", "root", &input.request_id, &session.id)
        .unwrap();
    let tree = Subagents::new(
        session.clone(),
        control.clone(),
        files.clone(),
        Arc::new(DoneExecutor),
        SubagentLimits::default(),
    );
    let model = ScopeModel::new(vec![
        vec![
            call(
                "grant",
                "subagent",
                json!({"prompt":"写一篇笔记","description":"写笔记","writeScope":["notes/"]}),
            ),
            call(
                "deny",
                "subagent",
                json!({"prompt":"越权","description":"越权","writeScope":["missing.md"]}),
            ),
            call("goal", "get_goal", json!({})),
        ],
        vec![call("list", "list_agents", json!({}))],
    ]);
    let cards = FakeCards::default();
    let result = tokio::time::timeout(Duration::from_secs(10), async {
        execute_with(
            AgentPorts {
                model: &model,
                repository: &*files,
                learning: &LEARNING,
                video: &VIDEO,
                dictionary: &DICTIONARY,
                cards: &cards,
            },
            &mut session,
            &input,
            &control,
            Some(tree.clone()),
            AgentExecutionSettings::default(),
        )
        .await
    })
    .await
    .unwrap();
    assert!(
        result.is_ok(),
        "根执行失败：{:?}",
        result.err().map(|e| e.code)
    );
    let reported = results(&session).join("\n");
    assert!(
        reported.contains("AGENT_WRITE_SCOPE_DENIED"),
        "不存在的授权路径必须被拒"
    );
    assert!(
        reported.contains("\"writeScope\":[\"notes/\"]"),
        "状态输出必须带出子代理授权"
    );
    assert!(
        reported.contains("\"writeScope\":[]"),
        "get_goal 必须带出本会话授权"
    );
    let children = ChildRepository::list(&*files, &session.id).unwrap();
    assert_eq!(children.len(), 1, "被拒的派生不得留下会话");
    assert_eq!(children[0].write_scope, vec!["notes/".to_string()]);
    tree.shutdown().await;
}
