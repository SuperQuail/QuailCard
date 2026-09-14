use super::*;

#[tokio::test]
/// 中断保留排队消息但不自启；后续 send 才能重新准入且每轮执行身份不同。
async fn interrupted_messages_stay_parked_until_send() {
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
    let first = receiver.recv().await.unwrap();
    manager.send("root", &child, "parked").await.unwrap();
    manager.interrupt("root", &child).unwrap();
    idle(&manager, &child).await;
    assert!(receiver.try_recv().is_err());
    assert!(!manager.has_pending(&child));
    assert!(manager.drain(&child).is_empty());
    manager.send("root", &child, "resume").await.unwrap();
    let next = receiver.recv().await.unwrap();
    assert_eq!(next.execution.prompt, "parked");
    assert_ne!(first.execution.execution_id, next.execution.execution_id);
    assert_eq!(manager.drain(&child)[0].content, "resume");
    manager.shutdown().await;
}

#[tokio::test]
/// 停止后即使执行器才交回结果，也不能重新投递通知或准入任何工作。
async fn shutdown_joins_and_closes_admission() {
    let parent = root();
    let repo = Arc::new(FakeRepo::default());
    let (manager, mut receiver) = setup(parent.clone(), repo.clone(), SubagentLimits::default());
    let child = manager
        .spawn("root", &parent, "first", "child", false, vec![])
        .await
        .unwrap();
    let run = receiver.recv().await.unwrap();
    manager.shutdown().await;
    assert!(run.control.is_cancelled());
    assert!(!manager.root_control.is_cancelled());
    assert_eq!(repo.load(&child).unwrap().messages.len(), 1);
    assert!(!manager.has_pending("root"));
    assert!(manager.drain("root").is_empty());
    assert!(manager.send("root", &child, "late").await.is_err());
    assert!(manager
        .spawn("root", &parent, "late", "late", false, vec![])
        .await
        .is_err());
    assert!(manager.acquire_model().await.is_err());
    manager.wait("root").await;
    manager.shutdown().await;
}

#[tokio::test]
/// 根取消只读取标记：不会额外消费其取消通知，也不会再投递孩子完成消息。
async fn root_cancel_suppresses_late_results() {
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
    manager.root_control.cancel();
    assert!(manager.send("root", &child, "late").await.is_err());
    run.reply.send("迟到完成".into()).unwrap();
    manager.shutdown().await;
    assert!(manager.drain("root").is_empty());
}

#[tokio::test]
/// 稳定身份数量包含 idle，伪造快照路径和 caller 身份均不能扩大授权。
async fn idle_identity_quota_and_snapshot_scope() {
    let mut parent = root();
    let limits = SubagentLimits {
        max_agents: 1,
        ..Default::default()
    };
    let (manager, mut receiver) = setup(parent.clone(), Arc::new(FakeRepo::default()), limits);
    parent.selected_paths.push("outside.md".into());
    assert!(manager
        .spawn(
            "root",
            &parent,
            "bad",
            "bad",
            false,
            vec!["outside.md".into()]
        )
        .await
        .is_err());
    assert!(manager
        .spawn("unknown", &parent, "bad", "bad", false, vec![])
        .await
        .is_err());
    let child = manager
        .spawn("root", &parent, "first", "child", false, vec![])
        .await
        .unwrap();
    let run = receiver.recv().await.unwrap();
    run.reply.send("done".into()).unwrap();
    idle(&manager, &child).await;
    assert!(manager
        .spawn("root", &parent, "second", "child", false, vec![])
        .await
        .is_err());
    manager.shutdown().await;
}

#[tokio::test]
/// 多字节结果仅在字符边界截断，完成通知不挤占用户消息额度。
async fn reports_are_bounded_and_wake_waiters() {
    let parent = root();
    let limits = SubagentLimits {
        max_message_bytes: 7,
        max_messages: 1,
        ..Default::default()
    };
    let (manager, mut receiver) = setup(parent.clone(), Arc::new(FakeRepo::default()), limits);
    let child = manager
        .spawn("root", &parent, "task", "child", false, vec![])
        .await
        .unwrap();
    let run = receiver.recv().await.unwrap();
    run.reply.send("中文结果非常长".into()).unwrap();
    manager.wait("root").await;
    let notifications = manager.drain("root");
    assert_eq!(notifications.len(), 1);
    assert_eq!(notifications[0].kind, "completed");
    assert_eq!(notifications[0].agent_id, child);
    assert_eq!(notifications[0].content, "中文");
    manager.shutdown().await;
}

#[tokio::test]
/// 关闭时释放准入锁再 join，孩子卡在递归 spawn 不会与 shutdown 互等。
async fn shutdown_does_not_hold_admission_while_joining() {
    let parent = root();
    let (manager, mut receiver) = setup(
        parent.clone(),
        Arc::new(FakeRepo::default()),
        SubagentLimits::default(),
    );
    let _child = manager
        .spawn("root", &parent, "task", "child", false, vec![])
        .await
        .unwrap();
    let run = receiver.recv().await.unwrap();
    let admission = manager.admission.lock().await;
    let closing = manager.clone();
    let shutdown = tokio::spawn(async move {
        closing.shutdown().await;
    });
    run.control.cancelled().await;
    drop(admission);
    shutdown.await.unwrap();
}
