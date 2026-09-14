use super::*;
use futures_util::FutureExt;
use std::time::Duration;

/// 单槽树暴露排队与释放行为，不依赖真实模型或墙钟等待。
fn single_slot() -> Arc<Subagents> {
    setup(
        root(),
        Arc::new(FakeRepo::default()),
        SubagentLimits {
            max_concurrent_models: 1,
            ..Default::default()
        },
    )
    .0
}

#[tokio::test]
/// 超过旧整树 256 次额度后仍可请求、派生和发送，模型槽不变成累计计数。
async fn calls_beyond_old_limit_keep_work_admission_open() {
    let parent = root();
    let (manager, mut receiver) = setup(
        parent.clone(),
        Arc::new(FakeRepo::default()),
        SubagentLimits {
            max_concurrent_models: 1,
            ..Default::default()
        },
    );
    for _ in 0..512 {
        drop(manager.acquire_model().await.unwrap());
    }
    let parent_permit = manager.acquire_model().await.unwrap();
    let child = manager
        .spawn("root", &parent, "task", "child", false, vec![])
        .await
        .unwrap();
    let _run = receiver.recv().await.unwrap();
    let mut child_permit = Box::pin(manager.acquire_model());
    assert!(child_permit.as_mut().now_or_never().is_none());
    manager.send("root", &child, "next").await.unwrap();
    drop(parent_permit);
    drop(child_permit.await.unwrap());
    assert_eq!(manager.list("root", false).unwrap()[0].agent_id, child);
    manager.shutdown().await;
}

#[tokio::test(start_paused = true)]
/// 虚拟时钟越过旧十五分钟后仍只等待并发槽，释放后可以继续调用和派生。
async fn permit_wait_and_new_work_outlive_old_deadline() {
    let parent = root();
    let (manager, mut receiver) = setup(
        parent.clone(),
        Arc::new(FakeRepo::default()),
        SubagentLimits {
            max_concurrent_models: 1,
            ..Default::default()
        },
    );
    let permit = manager.acquire_model().await.unwrap();
    let mut waiting = Box::pin(manager.acquire_model());
    assert!(waiting.as_mut().now_or_never().is_none());
    tokio::time::advance(Duration::from_secs(901)).await;
    assert!(waiting.as_mut().now_or_never().is_none());
    drop(permit);
    drop(waiting.await.unwrap());
    drop(manager.acquire_model().await.unwrap());
    let child = manager
        .spawn("root", &parent, "task", "child", false, vec![])
        .await
        .unwrap();
    let _run = receiver.recv().await.unwrap();
    manager.send("root", &child, "next").await.unwrap();
    manager.shutdown().await;
}

#[tokio::test]
/// 关闭必须唤醒所有排队者，即便正在使用的模型槽尚未释放。
async fn shutdown_wakes_queued_model_calls() {
    let manager = single_slot();
    let _permit = manager.acquire_model().await.unwrap();
    let mut first = Box::pin(manager.acquire_model());
    let mut second = Box::pin(manager.acquire_model());
    assert!(first.as_mut().now_or_never().is_none());
    assert!(second.as_mut().now_or_never().is_none());
    manager.shutdown().await;
    assert_eq!(first.await.unwrap_err().code, "AGENT_CANCELLED");
    assert_eq!(second.await.unwrap_err().code, "AGENT_CANCELLED");
    assert_eq!(
        manager.acquire_model().await.unwrap_err().code,
        "AGENT_CANCELLED"
    );
}

#[tokio::test]
/// 根在排队期间取消后，取到槽也必须复验拒绝，并归还许可而非开始新调用。
async fn queued_model_call_rechecks_root_cancellation() {
    let manager = single_slot();
    let permit = manager.acquire_model().await.unwrap();
    let mut waiting = Box::pin(manager.acquire_model());
    assert!(waiting.as_mut().now_or_never().is_none());
    manager.root_control.cancel();
    drop(permit);
    assert_eq!(waiting.await.unwrap_err().code, "AGENT_CANCELLED");
    assert_eq!(manager.models.available_permits(), 1);
    assert_eq!(
        manager.acquire_model().await.unwrap_err().code,
        "AGENT_CANCELLED"
    );
    manager.shutdown().await;
}

#[tokio::test(start_paused = true)]
/// 调用侧控制器的取消分支可丢弃无时限排队，不能占用后续调用的许可。
async fn caller_cancellation_ends_wait_without_releasing_held_slot() {
    let manager = single_slot();
    let permit = manager.acquire_model().await.unwrap();
    let (control, _) = AgentTasks::default()
        .register("window", "vault", "child-run", "child")
        .unwrap();
    // 与 agent_step 的许可等待契约一致，使用当前节点而非竞争根的取消通知。
    let mut waiting = Box::pin(async {
        tokio::select! { biased;
            _ = control.cancelled() => Err(stopped()),
            permit = manager.acquire_model() => permit,
        }
    });
    assert!(waiting.as_mut().now_or_never().is_none());
    tokio::time::advance(Duration::from_secs(901)).await;
    assert!(waiting.as_mut().now_or_never().is_none());
    control.cancel();
    assert_eq!(waiting.await.unwrap_err().code, "AGENT_CANCELLED");
    assert_eq!(manager.models.available_permits(), 0);
    drop(permit);
    drop(manager.acquire_model().await.unwrap());
    manager.shutdown().await;
}
