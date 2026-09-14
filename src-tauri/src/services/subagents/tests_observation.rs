use super::*;

struct NoReads;
impl ChildRepository for NoReads {
    /// 纯内存观察不能创建磁盘身份。
    fn create(&self, _: &AgentSession) -> Result<(), CommandError> {
        panic!("观察不可创建");
    }
    /// 纯内存观察不能冷加载历史。
    fn load(&self, _: &str) -> Result<AgentSession, CommandError> {
        panic!("观察不可读盘");
    }
    /// 纯内存观察不能触发目录枚举。
    fn list(&self, _: &str) -> Result<Vec<AgentSession>, CommandError> {
        panic!("观察不可扫描");
    }
    /// 纯内存观察甚至不能初始化快照。
    fn snapshot(&self) -> Result<Option<ChildSnapshot>, CommandError> {
        panic!("观察不可取快照");
    }
}

/// 手工登记真实内存父边和运行控制，不依赖执行调度时机。
fn add_node(manager: &Subagents, id: &str, parent: &str, depth: u32) -> Arc<AgentControl> {
    let (control, _) = AgentTasks::default()
        .register("window", "vault", &format!("run-{id}"), id)
        .unwrap();
    let mut node = Node::from_session(&AgentSession {
        id: id.into(),
        parent_session_id: Some(parent.into()),
        delegation_depth: depth,
        ..Default::default()
    });
    node.control = Some(control.clone());
    node.running = true;
    manager.lock().nodes.insert(id.into(), node);
    control
}

#[tokio::test]
/// 观察能读取深层后代，却不能观察自身、祖先或兄弟；只读操作不依赖模型调用。
async fn observation_is_memory_only_and_descendant_scoped() {
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
        control.clone(),
        Arc::new(NoReads),
        executor,
        SubagentLimits::default(),
    );
    add_node(&manager, "a", "root", 1);
    let child_control = add_node(&manager, "b", "a", 2);
    add_node(&manager, "sibling", "root", 1);
    assert_eq!(
        manager.observe_run("root", "b").unwrap().unwrap().id,
        "run-b"
    );
    assert!(manager.observe_run("a", "b").unwrap().is_some());
    assert!(manager.observe_run("a", "sibling").is_err());
    assert!(manager.observe_run("b", "a").is_err());
    assert!(manager.observe_run("root", "root").is_err());
    assert!(manager.observe_run("foreign", "b").is_err());
    assert!(manager.observe_run("root", "cold").unwrap().is_none());
    child_control.complete(None);
    assert_eq!(
        manager.observe_run("root", "b").unwrap().unwrap().state,
        "completed"
    );
    manager.lock().nodes.get_mut("b").unwrap().depth = 99;
    assert!(manager.observe_run("root", "b").is_err());
    manager.lock().nodes.get_mut("b").unwrap().depth = 2;
    control.cancel();
    assert!(manager.observe_run("root", "b").unwrap().is_none());
    manager.shutdown().await;
    assert!(manager.observe_run("root", "b").unwrap().is_none());
}
