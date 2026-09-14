use super::*;

#[tokio::test]
/// peek 可重复读取；旧批次 ack 不得吞掉持久化期间新到达的父子消息。
async fn peek_is_non_destructive_and_ack_removes_only_exact_ids() {
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
    let _run = receiver.recv().await.unwrap();
    let first = manager.send(&child, "root", "first").await.unwrap();
    let viewed = manager.peek("root");
    assert_eq!(viewed.len(), 1);
    assert_eq!(viewed[0].message_id, first);
    assert_eq!(manager.peek("root")[0].message_id, first);
    let second = manager.send(&child, "root", "second").await.unwrap();
    manager.ack("root", &[first.clone(), "unknown-id".into()]);
    assert_eq!(manager.peek("root").len(), 1);
    assert_eq!(manager.peek("root")[0].message_id, second);
    manager.ack("root", &[first]);
    assert_eq!(manager.peek("root")[0].message_id, second);
    manager.ack("root", &[second]);
    assert!(manager.peek("root").is_empty());
    manager.shutdown().await;
}

#[tokio::test]
/// 同一子的新完成报告有新 message_id，旧执行收据不能误确认替换后的新报告。
async fn old_ack_does_not_remove_new_completion_from_same_child() {
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
    run.reply.send("first report".into()).unwrap();
    manager.wait("root").await;
    let old = manager.peek("root")[0].clone();
    manager.send("root", &child, "second").await.unwrap();
    let next = receiver.recv().await.unwrap();
    // 使用下一报告自己的事件等待，不让尚未确认的旧报告造成 wait 立即返回。
    let changed = manager.lock().nodes[&child].changed.clone();
    let event = changed.notified();
    tokio::pin!(event);
    event.as_mut().enable();
    next.reply.send("second report".into()).unwrap();
    event.await;
    let fresh = manager.peek("root");
    assert_eq!(fresh.len(), 1);
    assert_ne!(fresh[0].message_id, old.message_id);
    assert_ne!(fresh[0].execution_id, old.execution_id);
    manager.ack("root", &[old.message_id]);
    assert_eq!(manager.peek("root")[0].message_id, fresh[0].message_id);
    manager.shutdown().await;
}
