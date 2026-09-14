use super::*;
use futures_util::FutureExt;

/// 保存好的直接子可在终态冻结前冷加载，测试不依赖当前运行任务。
fn saved_child(repo: &FakeRepo) {
    repo.create(&AgentSession {
        id: "saved".into(),
        parent_session_id: Some("root".into()),
        delegation_depth: 1,
        selected_paths: root().selected_paths,
        ..Default::default()
    })
    .unwrap();
}

#[tokio::test]
/// 完成需要原子静默：活动后代与未读根消息都拒绝，子节点不能冻结根。
async fn quiet_seal_rejects_pending_and_non_root() {
    let parent = root();
    let (manager, mut receiver) = setup(
        parent.clone(),
        Arc::new(FakeRepo::default()),
        SubagentLimits::default(),
    );
    let child = manager
        .spawn("root", &parent, "task", "child", false, vec![])
        .await
        .unwrap();
    let run = receiver.recv().await.unwrap();
    assert!(manager.seal_terminal(&child, false).is_err());
    assert_eq!(
        manager.seal_terminal("root", true).err().unwrap().code,
        "SUBAGENT_PENDING"
    );
    run.reply.send("done".into()).unwrap();
    manager.wait("root").await;
    assert!(!manager.lock().nodes[&child].running);
    assert_eq!(
        manager.seal_terminal("root", true).err().unwrap().code,
        "SUBAGENT_PENDING"
    );
    manager.drain("root");
    drop(manager.seal_terminal("root", true).unwrap());
    manager.shutdown().await;
}

#[tokio::test]
/// 冻结阻止派生和发送，但不阻止根的模型总结、列表或其他只读终态检查。
async fn seal_blocks_work_but_allows_model_and_list() {
    let parent = root();
    let repo = Arc::new(FakeRepo::default());
    saved_child(&repo);
    let (manager, mut receiver) = setup(parent.clone(), repo, SubagentLimits::default());
    let lease = manager.seal_terminal("root", true).unwrap();
    assert!(manager.send("root", "saved", "late").await.is_err());
    assert!(manager
        .spawn("root", &parent, "late", "late", false, vec![])
        .await
        .is_err());
    drop(manager.acquire_model().await.unwrap());
    assert_eq!(manager.list("root", false).unwrap()[0].agent_id, "saved");
    assert!(receiver.try_recv().is_err());
    drop(lease);
    manager.shutdown().await;
}

#[tokio::test]
/// 保存失败丢弃租约可恢复发送；保存成功 commit 后冻结一直持续到 shutdown。
async fn dropping_lease_rolls_back_then_commit_keeps_sealed() {
    let parent = root();
    let repo = Arc::new(FakeRepo::default());
    saved_child(&repo);
    let (manager, mut receiver) = setup(parent.clone(), repo, SubagentLimits::default());
    {
        let _failed_save = manager.seal_terminal("root", true).unwrap();
        assert!(manager.seal_terminal("root", false).is_err());
    }
    manager.send("root", "saved", "accepted").await.unwrap();
    let run = receiver.recv().await.unwrap();
    manager.seal_terminal("root", false).unwrap().commit();
    assert!(manager.send("root", "saved", "late").await.is_err());
    assert!(manager
        .spawn("root", &parent, "late", "late", false, vec![])
        .await
        .is_err());
    assert!(manager.seal_terminal("root", false).is_err());
    assert!(!run.control.is_cancelled());
    manager.shutdown().await;
    assert!(run.control.is_cancelled());
}

#[tokio::test]
/// 预约在 await 前通过检查也不算接收，取得准入锁后必须看到新冻结。
async fn queued_send_and_spawn_recheck_after_admission_wait() {
    let parent = root();
    let repo = Arc::new(FakeRepo::default());
    saved_child(&repo);
    let (manager, mut receiver) = setup(parent.clone(), repo.clone(), SubagentLimits::default());
    let admission = manager.admission.lock().await;
    let mut send = Box::pin(manager.send("root", "saved", "queued"));
    let mut spawn = Box::pin(manager.spawn("root", &parent, "queued", "queued", false, vec![]));
    assert!(send.as_mut().now_or_never().is_none());
    assert!(spawn.as_mut().now_or_never().is_none());
    manager.seal_terminal("root", true).unwrap().commit();
    drop(admission);
    assert!(send.await.is_err());
    assert!(spawn.await.is_err());
    assert!(receiver.try_recv().is_err());
    assert_eq!(repo.list("root").unwrap().len(), 1);
    manager.shutdown().await;
}

#[tokio::test]
/// 暂停或阻塞可冻结已有活动树；孩子收尾不能再启动已排队的后续轮次。
async fn pause_seal_prevents_finish_from_launching_queued_work() {
    let parent = root();
    let (manager, mut receiver) = setup(
        parent.clone(),
        Arc::new(FakeRepo::default()),
        SubagentLimits::default(),
    );
    let child = manager
        .spawn("root", &parent, "first", "child", false, vec![])
        .await
        .unwrap();
    let run = receiver.recv().await.unwrap();
    manager.send("root", &child, "queued").await.unwrap();
    manager.seal_terminal("root", false).unwrap().commit();
    run.reply.send("done".into()).unwrap();
    manager.wait("root").await;
    assert!(!manager.lock().nodes[&child].running);
    assert!(receiver.try_recv().is_err());
    assert_eq!(manager.lock().nodes[&child].inbox.len(), 1);
    manager.shutdown().await;
}

#[tokio::test]
/// 多次模型调用后仍可提交终态；关闭后的 Drop 不能重新开放整树。
async fn repeated_calls_can_seal_and_shutdown_cannot_be_rolled_back() {
    let (manager, _) = setup(
        root(),
        Arc::new(FakeRepo::default()),
        SubagentLimits::default(),
    );
    for _ in 0..512 {
        drop(manager.acquire_model().await.unwrap());
    }
    let lease = manager.seal_terminal("root", true).unwrap();
    manager.shutdown().await;
    drop(lease);
    assert!(manager.seal_terminal("root", false).is_err());
    assert!(manager.lock().closed);
}
