use super::*;

/// 手工登记真实内存父边与运行控制；写入协调只依赖这些身份。
fn add_child(manager: &Subagents, id: &str, depth: u32) -> Arc<AgentControl> {
    let (control, _) = AgentTasks::default()
        .register("window", "vault", &format!("run-{id}"), id)
        .unwrap();
    let mut node = Node::from_session(&AgentSession {
        id: id.into(),
        parent_session_id: Some("root".into()),
        delegation_depth: depth,
        ..Default::default()
    });
    node.control = Some(control.clone());
    node.running = true;
    manager.lock().nodes.insert(id.into(), node);
    control
}

/// 最小执行树：身份真实，仓库与执行器都是纯内存假实现。
fn tree() -> (Arc<Subagents>, Arc<AgentControl>) {
    let (started, _receiver) = mpsc::unbounded_channel();
    let executor = Arc::new(FakeExecutor {
        repo: Arc::new(FakeRepo::default()),
        started,
    });
    let (control, _) = AgentTasks::default()
        .register("window", "vault", "run-root", "root")
        .unwrap();
    let manager = Subagents::new(
        root(),
        control.clone(),
        Arc::new(FakeRepo::default()),
        executor,
        SubagentLimits::default(),
    );
    (manager, control)
}

/// 等待标记发布，避免测试依赖调度时机。
async fn published(control: &AgentControl) {
    while control.pending_write().is_none() {
        tokio::task::yield_now().await;
    }
}

#[tokio::test]
/// 子代理的保存等待能被整树查询发现，并在确认后继续执行。
async fn child_write_is_visible_and_acknowledged_across_the_tree() {
    let (manager, _root_control) = tree();
    let child = add_child(&manager, "child", 1);
    let waiting = {
        let child = child.clone();
        tokio::spawn(async move { child.prepare_write("note.md", "operation").await })
    };
    published(&child).await;
    let writes = manager.pending_writes();
    assert_eq!(writes.len(), 1);
    assert_eq!(writes[0].execution_id, "run-child");
    assert_eq!(writes[0].session_id, "child");
    assert_eq!(writes[0].path, "note.md");
    assert_eq!(writes[0].operation, "operation");
    let acknowledging = {
        let manager = manager.clone();
        tokio::spawn(async move { manager.acknowledge_write("run-child", "operation").await })
    };
    assert!(waiting.await.unwrap().is_ok());
    child.update(|state| {
        state.pending_write = None;
        state.pending_write_id = None;
    });
    assert!(acknowledging.await.unwrap().is_ok());
    assert!(manager.pending_writes().is_empty());
}

#[tokio::test]
/// 根执行自身的保存等待同样经由整树查询，不再依赖单独命令。
async fn root_write_is_reported_by_the_same_query() {
    let (manager, root_control) = tree();
    add_child(&manager, "child", 1);
    let waiting = {
        let control = root_control.clone();
        tokio::spawn(async move { control.prepare_write("root.md", "root-operation").await })
    };
    published(&root_control).await;
    let writes = manager.pending_writes();
    assert_eq!(writes.len(), 1);
    assert_eq!(writes[0].execution_id, "run-root");
    assert_eq!(writes[0].path, "root.md");
    let acknowledging = {
        let manager = manager.clone();
        tokio::spawn(async move {
            manager
                .acknowledge_write("run-root", "root-operation")
                .await
        })
    };
    assert!(waiting.await.unwrap().is_ok());
    root_control.update(|state| {
        state.pending_write = None;
        state.pending_write_id = None;
    });
    assert!(acknowledging.await.unwrap().is_ok());
    assert!(manager.pending_writes().is_empty());
}

#[tokio::test]
/// 外来身份、过期操作与已停止的树都拒绝确认，等待方只能由取消唤醒。
async fn acknowledgement_rejects_foreign_stale_and_stopped_requests() {
    let (manager, root_control) = tree();
    let child = add_child(&manager, "child", 1);
    let waiting = {
        let child = child.clone();
        tokio::spawn(async move { child.prepare_write("note.md", "operation").await })
    };
    published(&child).await;
    assert!(manager
        .acknowledge_write("run-other", "operation")
        .await
        .is_err());
    assert!(manager
        .acknowledge_write("run-child", "stale")
        .await
        .is_err());
    assert_eq!(manager.pending_writes().len(), 1);
    child.cancel();
    assert!(waiting.await.unwrap().is_err());
    root_control.cancel();
    assert!(manager.pending_writes().is_empty());
    assert!(manager
        .acknowledge_write("run-child", "operation")
        .await
        .is_err());
}

#[tokio::test]
/// 并行写入按根优先与深度稳定排序，确认一个执行不推进其他执行。
async fn concurrent_writes_are_ordered_and_independent() {
    let (manager, root_control) = tree();
    let first = add_child(&manager, "a", 1);
    let second = add_child(&manager, "b", 2);
    let waiting_first = {
        let control = first.clone();
        tokio::spawn(async move { control.prepare_write("a.md", "op-a").await })
    };
    published(&first).await;
    let waiting_second = {
        let control = second.clone();
        tokio::spawn(async move { control.prepare_write("b.md", "op-b").await })
    };
    published(&second).await;
    let waiting_root = {
        let control = root_control.clone();
        tokio::spawn(async move { control.prepare_write("root.md", "op-root").await })
    };
    published(&root_control).await;
    let order = manager
        .pending_writes()
        .into_iter()
        .map(|write| write.execution_id)
        .collect::<Vec<_>>();
    assert_eq!(order, ["run-root", "run-a", "run-b"]);
    let acknowledging = {
        let manager = manager.clone();
        tokio::spawn(async move { manager.acknowledge_write("run-a", "op-a").await })
    };
    assert!(waiting_first.await.unwrap().is_ok());
    first.update(|state| {
        state.pending_write = None;
        state.pending_write_id = None;
    });
    assert!(acknowledging.await.unwrap().is_ok());
    assert_eq!(manager.pending_writes().len(), 2);
    assert!(!waiting_second.is_finished() && !waiting_root.is_finished());
    second.cancel();
    root_control.cancel();
    assert!(waiting_second.await.unwrap().is_err());
    assert!(waiting_root.await.unwrap().is_err());
}
