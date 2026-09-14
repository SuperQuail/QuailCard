//! 任务登记、取消、终态和进度的无网络回归测试。
use super::*;

/// 测试记录使用独立任务编号，不触碰用户知识库。
fn record(id: &str) -> VideoTaskRecord {
    VideoTaskRecord::new(id, "video", "source")
}

/// 失败装配的最后一步不留下运行槽，用户可立即重试。
#[test]
fn prepare_persistence_failure_releases_slot() {
    let registry = VideoTaskRegistry::default();
    let result = registry.register_persisted("owner", &record("failed"), || {
        Err(CommandError::validation("模拟保存失败"))
    });
    assert!(result.is_err());
    assert_eq!(registry.status("owner", "failed").unwrap().state, "failed");
    assert!(registry
        .register_persisted("owner", &record("retry"), || Ok(()))
        .is_ok());
}

/// 同窗口互斥先于保存，不能留下新的排队幽灵记录。
#[test]
fn duplicate_registration_does_not_write() {
    let registry = VideoTaskRegistry::default();
    registry.register("owner", &record("one")).unwrap();
    let called = AtomicBool::new(false);
    assert!(registry
        .register_persisted("owner", &record("two"), || {
            called.store(true, Ordering::SeqCst);
            Ok(())
        })
        .is_err());
    assert!(!called.load(Ordering::SeqCst));
}

/// 进度单调且有上界，终态不能被迟到回调覆盖；日志始终有界。
#[test]
fn progress_and_terminal_are_monotonic() {
    let control = VideoControl::new("one".into(), TaskOrigin::User);
    control.update(|s| s.progress = 90);
    control.update(|s| s.progress = 20);
    assert_eq!(control.snapshot().progress, 90);
    for i in 0..200 {
        control.update(|s| s.step = format!("步骤 {i}"));
    }
    assert_eq!(control.snapshot().logs.len(), 100);
    control.complete(&Ok(()));
    let finished = control.snapshot();
    control.update(|s| {
        s.progress = 1;
        s.state = "running".into();
    });
    assert_eq!(control.snapshot().state, "completed");
    assert_eq!(control.snapshot().sequence, finished.sequence);
    assert_eq!(control.snapshot().progress, 100);
}

/// 历史可见运行态，但跨窗口详细查询和取消能力仍被隔离。
#[test]
fn history_state_does_not_grant_task_access() {
    let registry = VideoTaskRegistry::default();
    let control = registry.register("owner", &record("one")).unwrap();
    assert_eq!(
        registry.live_state_for_history("one").as_deref(),
        Some("running")
    );
    assert!(registry.status("other", "one").is_err());
    registry.cancel("other", "one").unwrap();
    assert!(!control.is_cancelled());
}

/// Agent 进度只返回精确归属的运行任务，不因父子并行或终态保留串线。
#[test]
fn running_progress_is_owner_bound_and_excludes_terminal() {
    let registry = VideoTaskRegistry::default();
    let root = registry
        .register("agent:root", &record("root-task"))
        .unwrap();
    registry
        .register("agent:child", &record("child-task"))
        .unwrap();
    assert_eq!(
        registry.running_status("agent:root").unwrap().task_id,
        "root-task"
    );
    assert_eq!(
        registry.running_status("agent:child").unwrap().task_id,
        "child-task"
    );
    assert!(registry.running_status("agent:other").is_none());
    root.complete(&Ok(()));
    assert!(registry.running_status("agent:root").is_none());
}

/// 先取消再等待以及等待中取消都不能丢失通知。
#[tokio::test]
async fn cancellation_notification_has_no_lost_wakeup() {
    let control = VideoControl::new("one".into(), TaskOrigin::User);
    control.cancel();
    tokio::time::timeout(Duration::from_secs(1), control.cancelled())
        .await
        .unwrap();
    let control = VideoControl::new("two".into(), TaskOrigin::User);
    let sender = control.clone();
    let waiter = async {
        tokio::task::yield_now().await;
        sender.cancel();
    };
    tokio::join!(control.cancelled(), waiter);
    control.complete(&Err(CommandError::new("VIDEO_CANCELLED", "已停止")));
    assert_eq!(control.snapshot().state, "cancelled");
    control.complete(&Ok(()));
    assert_eq!(control.snapshot().state, "cancelled");
}
