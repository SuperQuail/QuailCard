use super::*;
use std::sync::atomic::{AtomicUsize, Ordering};

struct IndexedRepo {
    rows: Vec<AgentSession>,
    snapshots: AtomicUsize,
    cancel_on_snapshot: Option<Arc<AgentControl>>,
}
impl ChildRepository for IndexedRepo {
    /// 查询专用假仓库禁止隐式创建。
    fn create(&self, _: &AgentSession) -> Result<(), CommandError> {
        panic!("目录查询不可写入");
    }
    /// 有快照后不应按节点加载完整历史。
    fn load(&self, _: &str) -> Result<AgentSession, CommandError> {
        panic!("目录查询不可按节点加载历史");
    }
    /// 快照实现必须彻底替代逐节点的全库查询。
    fn list(&self, _: &str) -> Result<Vec<AgentSession>, CommandError> {
        panic!("已有快照不得回退 list");
    }
    /// 每次调用提供新快照，使测试能精确观察刷新边界。
    fn snapshot(&self) -> Result<Option<ChildSnapshot>, CommandError> {
        self.snapshots.fetch_add(1, Ordering::SeqCst);
        if let Some(control) = &self.cancel_on_snapshot {
            control.cancel();
        }
        ChildSnapshot::from_records(self.rows.iter().map(ChildRecord::from_session)).map(Some)
    }
}

/// 固定名称便于断言父子前序，不依赖 UUID 或枚举顺序。
fn saved(id: &str, parent: &str, depth: u32) -> AgentSession {
    AgentSession {
        id: id.into(),
        parent_session_id: Some(parent.into()),
        delegation_depth: depth,
        selected_paths: vec!["a.md".into()],
        ..Default::default()
    }
}

/// 快照计数从零开始，旧假仓库仍保持原来的三方法实现。
fn indexed(rows: Vec<AgentSession>) -> IndexedRepo {
    IndexedRepo {
        rows,
        snapshots: AtomicUsize::new(0),
        cancel_on_snapshot: None,
    }
}

#[test]
/// 默认 fake 回退和真实快照接口遵循同一稳定排序与权限契约。
fn snapshot_and_legacy_fake_share_listing_contract() {
    let rows = vec![
        saved("z", "root", 1),
        saved("a1", "a", 2),
        saved("a", "root", 1),
        saved("foreign", "other", 1),
    ];
    let fake = FakeRepo::default();
    for row in &rows {
        fake.create(row).unwrap();
    }
    let repo = indexed(rows);
    let cold = stored_children(&repo, &root()).unwrap();
    assert_eq!(repo.snapshots.load(Ordering::SeqCst), 1);
    assert_eq!(
        cold.iter().map(|n| n.agent_id.as_str()).collect::<Vec<_>>(),
        vec!["a", "a1", "z"]
    );
    assert_eq!(
        serde_json::to_value(&cold).unwrap(),
        serde_json::to_value(stored_children(&fake, &root()).unwrap()).unwrap()
    );
    assert_eq!(stored_children(&repo, &root()).unwrap().len(), 3);
    assert_eq!(repo.snapshots.load(Ordering::SeqCst), 2);
}

#[tokio::test]
/// 活动树同样只请求一次快照，并覆盖驻留状态，不把只读查询当作模型准入。
async fn active_tree_uses_one_snapshot_and_preserves_live_status() {
    let repo = Arc::new(indexed(vec![saved("a", "root", 1), saved("b", "a", 2)]));
    let (started, _receiver) = mpsc::unbounded_channel();
    let executor = Arc::new(FakeExecutor {
        repo: Arc::new(FakeRepo::default()),
        started,
    });
    let (control, _) = AgentTasks::default()
        .register("window", "vault", "run", "root")
        .unwrap();
    let manager = Subagents::new(
        root(),
        control,
        repo.clone(),
        executor,
        SubagentLimits::default(),
    );
    let mut node = Node::from_session(&saved("a", "root", 1));
    node.running = true;
    manager.lock().nodes.insert("a".into(), node);
    let listed = manager.list("root", true).unwrap();
    assert_eq!(repo.snapshots.load(Ordering::SeqCst), 1);
    assert_eq!(listed[0].status, "running");
    assert_eq!(listed[1].status, "ready");
    assert_eq!(manager.list("a", false).unwrap().len(), 1);
    assert_eq!(repo.snapshots.load(Ordering::SeqCst), 2);
    assert!(manager.list("foreign", true).is_err());
    assert_eq!(repo.snapshots.load(Ordering::SeqCst), 2);
    manager.shutdown().await;
}

#[test]
/// 深度溢出、循环、重复身份和输出上界均应显式失败，不能静默隐藏异常。
fn snapshots_reject_invalid_lineage_and_bounds() {
    let wrong_depth = indexed(vec![saved("a", "root", u32::MAX)]);
    assert!(stored_children(&wrong_depth, &root()).is_err());
    let cycle = indexed(vec![saved("a", "root", 1), saved("root", "a", 2)]);
    assert!(stored_children(&cycle, &root()).is_err());
    let duplicate = indexed(vec![saved("a", "root", 1), saved("a", "root", 1)]);
    assert!(stored_children(&duplicate, &root()).is_err());
    let wide = indexed(
        (0..256)
            .map(|n| saved(&format!("child-{n}"), "root", 1))
            .collect(),
    );
    assert!(stored_children(&wide, &root()).is_err());
    let mut invalid_root = root();
    invalid_root.parent_session_id = Some("other".into());
    assert!(stored_children(&indexed(vec![]), &invalid_root).is_err());
}

#[tokio::test]
/// 根后来收窄范围不能隐藏历史孩子；查询保留原写授权，但续聊仍须通过当前准入。
async fn historical_scopes_remain_listable_but_send_is_revalidated() {
    let mut parent = saved("a", "root", 1);
    parent.selected_paths.clear();
    let mut child = saved("b", "a", 2);
    child.write_scope = vec!["a.md".into()];
    let rows = vec![parent, child];
    let fake = Arc::new(FakeRepo::default());
    for row in &rows {
        fake.create(row).unwrap();
    }
    let repo = Arc::new(indexed(rows));
    let mut narrowed = root();
    narrowed.selected_paths = vec!["now.md".into()];
    narrowed.write_scope = vec!["now.md".into()];
    let cold = stored_children(repo.as_ref(), &narrowed).unwrap();
    assert_eq!(cold.len(), 2);
    assert_eq!(cold[1].write_scope, vec!["a.md"]);
    assert_eq!(
        serde_json::to_value(&cold).unwrap(),
        serde_json::to_value(stored_children(fake.as_ref(), &narrowed).unwrap()).unwrap()
    );
    let (started, _receiver) = mpsc::unbounded_channel();
    let executor = Arc::new(FakeExecutor {
        repo: fake.clone(),
        started,
    });
    let (control, _) = AgentTasks::default()
        .register("window", "vault", "run", "root")
        .unwrap();
    let manager = Subagents::new(
        narrowed.clone(),
        control,
        repo.clone(),
        executor,
        SubagentLimits::default(),
    );
    for listed in [
        manager.list("root", true).unwrap(),
        manager.observe_children("root").unwrap(),
    ] {
        assert_eq!(listed.len(), 2);
        assert_eq!(listed[1].write_scope, vec!["a.md"]);
    }
    assert_eq!(repo.snapshots.load(Ordering::SeqCst), 3);
    manager.shutdown().await;
    let (manager, _receiver) = setup(narrowed, fake, SubagentLimits::default());
    assert_eq!(manager.list("root", true).unwrap().len(), 2);
    assert!(manager.send("root", "a", "恢复旧任务").await.is_err());
    manager.shutdown().await;
}

struct WrongParentRepo;
impl ChildRepository for WrongParentRepo {
    /// 验证恶意实现时禁止创建以免掩盖查询问题。
    fn create(&self, _: &AgentSession) -> Result<(), CommandError> {
        panic!("不可创建");
    }
    /// 默认兼容路径只允许 list，不必在假仓库存储根。
    fn load(&self, _: &str) -> Result<AgentSession, CommandError> {
        panic!("不可加载");
    }
    /// 故意返回其他父级的节点，验证适配层不会盲信仓库列表。
    fn list(&self, _: &str) -> Result<Vec<AgentSession>, CommandError> {
        Ok(vec![saved("a", "foreign", 1)])
    }
}

#[test]
/// 兼容默认实现也必须验证权威父关系，不能以 fake 为由绕过权限。
fn fallback_rejects_wrong_parent() {
    assert!(stored_children(&WrongParentRepo, &root()).is_err());
}

#[tokio::test]
/// 扫描期间取消只把状态降级成 ready，不返回取消错误导致桥接层第二次扫描。
async fn observation_cancelled_during_snapshot_does_not_rescan() {
    let (control, _) = AgentTasks::default()
        .register("window", "vault", "run", "root")
        .unwrap();
    let mut repo = indexed(vec![saved("a", "root", 1)]);
    repo.cancel_on_snapshot = Some(control.clone());
    let repo = Arc::new(repo);
    let (started, _receiver) = mpsc::unbounded_channel();
    let executor = Arc::new(FakeExecutor {
        repo: Arc::new(FakeRepo::default()),
        started,
    });
    let manager = Subagents::new(
        root(),
        control,
        repo.clone(),
        executor,
        SubagentLimits::default(),
    );
    let mut node = Node::from_session(&saved("a", "root", 1));
    node.running = true;
    manager.lock().nodes.insert("a".into(), node);
    let observed = manager.observe_children("root").unwrap();
    assert_eq!(observed.len(), 1);
    assert_eq!(observed[0].status, "ready");
    assert_eq!(repo.snapshots.load(Ordering::SeqCst), 1);
    assert!(manager.list("root", true).is_err());
    assert_eq!(repo.snapshots.load(Ordering::SeqCst), 1);
    manager.shutdown().await;
}
