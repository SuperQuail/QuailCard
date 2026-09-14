//! 执行所有权回归：调用方消失不能中断 worker 的协作收尾。

use super::{wait, Execution};
use crate::{
    error::CommandError,
    services::video_tasks::{VideoControl, VideoTaskRegistry},
    storage::{
        testutil::TempDir,
        video::{VideoStorage, VideoTaskRecord},
    },
    video::transcript::{Segment, Transcript},
};
use std::{future::pending, path::PathBuf, time::Duration};
use tokio::{sync::oneshot, time::timeout};

const DEADLINE: Duration = Duration::from_secs(5);

/// 保持临时知识库句柄存活，所有持久化均经过真实存储层。
fn fixture() -> (TempDir, VideoStorage, VideoTaskRegistry) {
    let root = TempDir::new();
    let storage = VideoStorage::new(root.path());
    (root, storage, VideoTaskRegistry::default())
}

/// 按生产登记流程占槽并保存记录，避免测试绕过注册表生命周期。
fn register(
    storage: &VideoStorage,
    registry: &VideoTaskRegistry,
    owner: &str,
    id: &str,
) -> VideoControl {
    let mut record = VideoTaskRecord::new(id, "video-key", "https://example.com/video");
    record.state = "running".into();
    registry
        .register_persisted(owner, &record, || storage.save_task(&record))
        .unwrap()
}

/// 只向存储层校验过的路径写测试媒体，并建立应被保留的真实转录。
fn artifacts(storage: &VideoStorage, id: &str) -> (Transcript, Vec<PathBuf>) {
    let transcript = Transcript {
        language: "zh".into(),
        source: "bilibili_cc".into(),
        segments: vec![Segment {
            start: 0.0,
            end: 2.0,
            text: "取消生成笔记后仍应保留的转录".into(),
        }],
    };
    storage.save_transcript(id, &transcript).unwrap();
    let paths = vec![
        storage.media_file(id, "audio_p1.m4s").unwrap(),
        storage.media_file(id, "audio_p1.wav").unwrap(),
        storage.media_file(id, "video_p1.m4s").unwrap(),
        storage.shot_file(id, "shot-p1-1000.jpg").unwrap(),
    ];
    for path in &paths {
        std::fs::write(path, b"temporary media").unwrap();
    }
    (transcript, paths)
}

/// 使用显式同步确认 worker 已走到指定阶段，不依赖调度时长或 sleep。
async fn received<T>(receiver: oneshot::Receiver<T>) -> T {
    timeout(DEADLINE, receiver)
        .await
        .expect("worker 未在期限内到达同步点")
        .expect("worker 在发出同步信号前被丢弃")
}

/// 对比完整转录并逐项确认媒体删除，防止仅修改内存状态的伪收尾。
fn assert_cleaned(storage: &VideoStorage, id: &str, expected: &Transcript, media: &[PathBuf]) {
    assert_eq!(
        storage.load_transcript(id).unwrap().as_ref(),
        Some(expected)
    );
    assert!(media.iter().all(|path| !path.exists()));
}

/// 模拟流水线自己的终态落盘，Execution 不应改写这些业务字段。
fn persist_terminal(storage: &VideoStorage, id: &str, state: &str, error: Option<String>) {
    let mut record = storage.load_task(id).unwrap().unwrap();
    record.state = state.into();
    record.step = "流水线已收尾".into();
    record.error = error;
    record.note_path = Some("notes/existing.md".into());
    storage.save_task(&record).unwrap();
}

#[tokio::test]
/// 模拟 Agent select 丢弃等待分支，取消后 worker 必须继续 await 并真正清理落盘。
async fn dropped_wait_at_note_stage_keeps_worker_alive_until_cleanup() {
    let (_root, storage, registry) = fixture();
    let control = register(&storage, &registry, "agent", "cancel-note");
    let other = register(&storage, &registry, "other", "other-task");
    let other_before = serde_json::to_value(other.snapshot()).unwrap();
    let (transcript, media) = artifacts(&storage, "cancel-note");
    let (other_transcript, other_media) = artifacts(&storage, "other-task");
    let (ready_tx, ready_rx) = oneshot::channel();
    let (cancelled_tx, cancelled_rx) = oneshot::channel();
    let (cleanup_tx, cleanup_rx) = oneshot::channel();
    let (finished_tx, finished_rx) = oneshot::channel();
    let worker_control = control.clone();
    let worker_storage = storage.clone();
    let execution = Execution::new(control.clone(), storage.clone());
    let worker = async move {
        let mut execution = execution;
        worker_control.update(|status| {
            status.progress = 65;
            status.step = "生成笔记".into();
        });
        let mut record = worker_storage.load_task("cancel-note")?.unwrap();
        record.progress = 65;
        record.step = "生成笔记".into();
        worker_storage.save_task(&record)?;
        ready_tx.send(()).unwrap();
        worker_control.cancelled().await;
        cancelled_tx.send(()).unwrap();
        // 再次挂起证明 caller 消失后 worker 仍被独立任务持有，而非只运行了 Drop。
        cleanup_rx.await.unwrap();
        worker_storage.cleanup_task_media("cancel-note")?;
        let error = CommandError::new("VIDEO_CANCELLED", "视频任务已取消");
        persist_terminal(
            &worker_storage,
            "cancel-note",
            "cancelled",
            Some(error.message.clone()),
        );
        execution.finish(&Err(error.clone()));
        drop(execution);
        finished_tx.send(()).unwrap();
        Err(error)
    };
    let mut caller = Box::pin(wait(control.clone(), worker));
    timeout(DEADLINE, async {
        tokio::select! {
            signal = ready_rx => signal.expect("worker 未进入笔记阶段"),
            result = &mut caller => panic!("等待不应提前结束：{result:?}"),
        }
    })
    .await
    .expect("worker 未开始运行");
    assert_eq!(control.snapshot().progress, 65);
    assert!(!control.is_cancelled());
    drop(caller);
    received(cancelled_rx).await;
    assert!(control.is_cancelled());
    assert_eq!(control.snapshot().state, "running");
    assert!(media.iter().all(|path| path.exists()));
    let retry = VideoTaskRecord::new("retry-note", "key", "url");
    assert!(matches!(
        registry.register("agent", &retry),
        Err(error) if error.code == "VIDEO_TASK_RUNNING"
    ));
    cleanup_tx.send(()).unwrap();
    received(finished_rx).await;
    assert_eq!(
        registry.status("agent", "cancel-note").unwrap().state,
        "cancelled"
    );
    let saved = storage.load_task("cancel-note").unwrap().unwrap();
    assert_eq!(saved.state, "cancelled");
    assert_eq!(saved.step, "流水线已收尾");
    assert_eq!(saved.progress, 65);
    assert_eq!(saved.error.as_deref(), Some("视频任务已取消"));
    assert_cleaned(&storage, "cancel-note", &transcript, &media);
    assert!(registry.register("agent", &retry).is_ok());
    assert!(!other.is_cancelled());
    assert_eq!(
        serde_json::to_value(other.snapshot()).unwrap(),
        other_before
    );
    assert_eq!(
        storage.load_task("other-task").unwrap().unwrap().state,
        "running"
    );
    assert_eq!(
        storage.load_transcript("other-task").unwrap(),
        Some(other_transcript)
    );
    assert!(other_media.iter().all(|path| path.exists()));
}

#[test]
/// 任务尚未首次 poll 就被丢弃时也必须补存终态，且区分预先取消与意外中断。
fn unpolled_execution_releases_slot_and_persists_terminal() {
    for (cancel_first, expected) in [(false, "failed"), (true, "cancelled")] {
        let (_root, storage, registry) = fixture();
        let control = register(&storage, &registry, "agent", "unpolled");
        let (transcript, media) = artifacts(&storage, "unpolled");
        let execution = Execution::new(control.clone(), storage.clone());
        if cancel_first {
            control.cancel();
        }
        let future = async move {
            pending::<()>().await;
            drop(execution);
        };
        drop(future);
        assert!(control.is_cancelled());
        assert_eq!(control.snapshot().state, expected);
        let saved = storage.load_task("unpolled").unwrap().unwrap();
        assert_eq!(saved.state, expected);
        assert!(saved
            .error
            .as_ref()
            .is_some_and(|message| !message.is_empty()));
        assert_cleaned(&storage, "unpolled", &transcript, &media);
        register(&storage, &registry, "agent", "after-unpolled");
    }
}

#[tokio::test]
/// worker panic 由独立任务边界转成安全错误，执行守卫仍释放槽并保存失败。
async fn panicking_worker_persists_failure_without_leaking_payload() {
    let (_root, storage, registry) = fixture();
    let control = register(&storage, &registry, "agent", "panic-task");
    let (transcript, media) = artifacts(&storage, "panic-task");
    let execution = Execution::new(control.clone(), storage.clone());
    let worker = async move {
        let _execution = execution;
        panic!("secret-panic-payload https://provider.invalid/?api_key=hidden");
    };
    let error = timeout(DEADLINE, wait(control.clone(), worker))
        .await
        .expect("panic 后等待没有结束")
        .unwrap_err();
    assert_eq!(error.code, "VIDEO_INTERRUPTED");
    assert!(!error.message.is_empty());
    let exposed = serde_json::to_string(&error).unwrap();
    let saved = storage.load_task("panic-task").unwrap().unwrap();
    let persisted = serde_json::to_string(&saved).unwrap();
    for secret in ["secret-panic-payload", "provider.invalid", "api_key=hidden"] {
        assert!(!exposed.contains(secret));
        assert!(!persisted.contains(secret));
        assert!(!serde_json::to_string(&control.snapshot())
            .unwrap()
            .contains(secret));
    }
    assert_eq!(control.snapshot().state, "failed");
    assert_eq!(saved.state, "failed");
    assert!(saved.error.is_some());
    assert_cleaned(&storage, "panic-task", &transcript, &media);
    register(&storage, &registry, "agent", "after-panic");
}

#[tokio::test]
/// 正常成功和业务错误不会触发取消，重复 finish 与析构不能改变既有终态。
async fn normal_finish_is_idempotent_and_wait_does_not_cancel() {
    for fail in [false, true] {
        let (_root, storage, registry) = fixture();
        let control = register(&storage, &registry, "agent", "normal");
        let worker_control = control.clone();
        let worker_storage = storage.clone();
        let mut execution = Execution::new(control.clone(), storage.clone());
        let worker = async move {
            let result = if fail {
                Err(CommandError::new(
                    "VIDEO_NOTE_FAILED",
                    "生成笔记失败，请重试",
                ))
            } else {
                Ok(())
            };
            persist_terminal(
                &worker_storage,
                "normal",
                if fail { "failed" } else { "completed" },
                result.as_ref().err().map(|error| error.message.clone()),
            );
            let saved = serde_json::to_value(worker_storage.load_task("normal").unwrap()).unwrap();
            execution.finish(&result);
            let terminal = serde_json::to_value(worker_control.snapshot()).unwrap();
            execution.finish(&result);
            execution.finish(&Err(CommandError::new("VIDEO_CANCELLED", "迟到的取消")));
            drop(execution);
            assert_eq!(
                serde_json::to_value(worker_control.snapshot()).unwrap(),
                terminal
            );
            assert_eq!(
                serde_json::to_value(worker_storage.load_task("normal").unwrap()).unwrap(),
                saved
            );
            result?;
            Ok(worker_control.snapshot())
        };
        let result = timeout(DEADLINE, wait(control.clone(), worker))
            .await
            .unwrap();
        if fail {
            assert_eq!(result.unwrap_err().code, "VIDEO_NOTE_FAILED");
            assert_eq!(control.snapshot().state, "failed");
        } else {
            assert_eq!(result.unwrap().state, "completed");
        }
        assert!(!control.is_cancelled());
        register(&storage, &registry, "agent", "after-normal");
    }
}

#[test]
/// 普通控制句柄的析构不拥有任务终结权，不能取消任务或提前释放窗口槽。
fn dropping_control_clone_does_not_terminate_execution() {
    let (_root, storage, registry) = fixture();
    let control = register(&storage, &registry, "agent", "clone-task");
    let execution = Execution::new(control.clone(), storage.clone());
    let before = serde_json::to_value(control.snapshot()).unwrap();
    drop(control.clone());
    assert!(!control.is_cancelled());
    assert_eq!(serde_json::to_value(control.snapshot()).unwrap(), before);
    assert_eq!(
        storage.load_task("clone-task").unwrap().unwrap().state,
        "running"
    );
    let next = VideoTaskRecord::new("next-clone", "key", "url");
    assert!(matches!(
        registry.register("agent", &next),
        Err(error) if error.code == "VIDEO_TASK_RUNNING"
    ));
    drop(execution);
    assert!(registry.register("agent", &next).is_ok());
}

#[test]
/// 异常析构也不能覆盖流水线已经保存的任一种终态及笔记引用。
fn unfinished_drop_preserves_existing_terminal_records() {
    for state in ["completed", "failed", "cancelled"] {
        let (_root, storage, registry) = fixture();
        let control = register(&storage, &registry, "agent", "already-saved");
        let (transcript, media) = artifacts(&storage, "already-saved");
        let execution = Execution::new(control, storage.clone());
        persist_terminal(
            &storage,
            "already-saved",
            state,
            Some("已有安全消息".into()),
        );
        let before = serde_json::to_value(storage.load_task("already-saved").unwrap()).unwrap();
        drop(execution);
        assert_eq!(
            serde_json::to_value(storage.load_task("already-saved").unwrap()).unwrap(),
            before
        );
        assert_cleaned(&storage, "already-saved", &transcript, &media);
        register(&storage, &registry, "agent", "after-saved");
    }
}
