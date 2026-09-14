//! 候选抽帧的离线回归：用假操作验证顺序、上限、取消和资源回收。
use super::{try_candidates, Temporary, FRAME_CANDIDATE_TIMEOUT};
use crate::{error::CommandError, video::media::CancelHandle};
use std::{
    future::ready,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
};

/// 测试只传递标识符，防止意外依赖网络或真实签名。
fn urls(count: usize) -> Vec<String> {
    (0..count)
        .map(|index| format!("candidate-{index}"))
        .collect()
}

/// 返回不取消的共享句柄，让失败策略与取消策略分别测试。
fn active() -> CancelHandle {
    Arc::new(|| false)
}

/// 主地址失败后从头执行备用操作，成功后不访问更多候选。
#[tokio::test]
async fn first_failure_then_success() {
    let mut seen = Vec::new();
    let result = try_candidates(urls(3), &active(), |url, _attempt_cancel| {
        seen.push(url);
        ready(if seen.len() == 1 {
            Err(CommandError::new("VIDEO_FRAME_FAILED", "首个来源失败"))
        } else {
            Ok(())
        })
    })
    .await;
    assert!(result.is_ok());
    assert_eq!(seen, urls(2));
}

/// 耗尽后保留最后一次安全错误，不把失败误判为成功。
#[tokio::test]
async fn exhaustion_returns_last_failure() {
    let mut seen = Vec::new();
    let error = try_candidates(urls(3), &active(), |url, _attempt_cancel| {
        seen.push(url);
        ready(Err(CommandError::new(
            "VIDEO_FRAME_FAILED",
            format!("失败{}", seen.len()),
        )))
    })
    .await
    .unwrap_err();
    assert_eq!(seen, urls(3));
    assert_eq!(error.code, "VIDEO_FRAME_FAILED");
    assert_eq!(error.message, "失败3");
}

/// 即使第五个来源可用也不能绕过四次硬上限。
#[tokio::test]
async fn candidate_cap_is_four() {
    let mut seen = Vec::new();
    let result = try_candidates(urls(7), &active(), |url, _attempt_cancel| {
        seen.push(url);
        ready(if seen.len() > 4 {
            Ok(())
        } else {
            Err(CommandError::new("VIDEO_FRAME_FAILED", "失败"))
        })
    })
    .await;
    assert!(result.is_err());
    assert_eq!(seen, urls(4));
}

/// 空列表不启动操作，并返回可展示的错误。
#[tokio::test]
async fn empty_candidates_do_not_start_operation() {
    let error = try_candidates(Vec::new(), &active(), |_, _attempt_cancel| {
        panic!("空列表不应调用操作");
        #[allow(unreachable_code)]
        ready(Ok(()))
    })
    .await
    .unwrap_err();
    assert_eq!(error.code, "VIDEO_FRAME_FAILED");
}

/// 已取消的任务连首个候选也不能启动。
#[tokio::test]
async fn cancellation_before_attempt_does_not_start() {
    let cancel: CancelHandle = Arc::new(|| true);
    let mut calls = 0;
    let error = try_candidates(urls(2), &cancel, |_, _attempt_cancel| {
        calls += 1;
        ready(Ok(()))
    })
    .await
    .unwrap_err();
    assert_eq!(error.code, "VIDEO_CANCELLED");
    assert_eq!(calls, 0);
}

/// 组件直接报告取消或缺失时，备用地址不能修复该错误。
#[tokio::test]
async fn terminal_errors_never_retry() {
    for code in ["VIDEO_CANCELLED", "VIDEO_COMPONENT_MISSING"] {
        let mut calls = 0;
        let error = try_candidates(urls(3), &active(), |_, _attempt_cancel| {
            calls += 1;
            ready(Err(CommandError::new(code, "终止")))
        })
        .await
        .unwrap_err();
        assert_eq!(error.code, code);
        assert_eq!(calls, 1);
    }
}

/// 普通失败同时遇到取消标记时，取消优先且不启动备用来源。
#[tokio::test]
async fn cancellation_after_failure_prevents_retry() {
    let flag = Arc::new(AtomicBool::new(false));
    let observed = flag.clone();
    let cancel: CancelHandle = Arc::new(move || observed.load(Ordering::SeqCst));
    let mut calls = 0;
    let error = try_candidates(urls(2), &cancel, |_, _attempt_cancel| {
        calls += 1;
        flag.store(true, Ordering::SeqCst);
        ready(Err(CommandError::new("VIDEO_FRAME_FAILED", "失败")))
    })
    .await
    .unwrap_err();
    assert_eq!(error.code, "VIDEO_CANCELLED");
    assert_eq!(calls, 1);
}

/// 挂起操作也响应取消；虚拟时钟避免测试真实等待。
#[tokio::test(start_paused = true)]
async fn cancellation_during_pending_operation_never_retries() {
    let flag = Arc::new(AtomicBool::new(false));
    let observed = flag.clone();
    let cancel: CancelHandle = Arc::new(move || observed.load(Ordering::SeqCst));
    let mut calls = 0;
    let error = try_candidates(urls(2), &cancel, |_, _attempt_cancel| {
        calls += 1;
        let flag = flag.clone();
        async move {
            flag.store(true, Ordering::SeqCst);
            super::super::wait_for_cancel(&_attempt_cancel).await;
            // 模拟组件收尾；父操作必须等待，不能在取消时丢弃。
            tokio::time::sleep(std::time::Duration::from_millis(100)).await;
            Err(CommandError::new("VIDEO_CANCELLED", "取消"))
        }
    })
    .await
    .unwrap_err();
    assert_eq!(error.code, "VIDEO_CANCELLED");
    assert_eq!(calls, 1);
}

/// 超时先协作停止首个完整尝试并清理临时输出，备用来源才开始。
#[tokio::test(start_paused = true)]
async fn timeout_cleans_temporary_before_backup() {
    let output = std::env::temp_dir().join(format!("qc-retry-{}.jpg", uuid::Uuid::now_v7()));
    let frame = Temporary::beside(&output, "jpg");
    let temporary = frame.0.clone();
    std::fs::write(&temporary, b"partial").unwrap();
    let mut first = Some(frame);
    let mut calls = 0;
    let start = tokio::time::Instant::now();
    let result = try_candidates(urls(2), &active(), |_, _attempt_cancel| {
        calls += 1;
        let frame = first.take();
        let temporary = temporary.clone();
        async move {
            if let Some(_frame) = frame {
                super::super::wait_for_cancel(&_attempt_cancel).await;
                // 模拟进程回收延迟，确保编排层等待清理完成才启动下一次。
                tokio::time::sleep(std::time::Duration::from_millis(100)).await;
                return Err(CommandError::new("VIDEO_CANCELLED", "超时取消"));
            }
            assert!(!temporary.exists());
            Ok(())
        }
    })
    .await;
    assert!(result.is_ok());
    assert_eq!(calls, 2);
    assert!(start.elapsed() >= FRAME_CANDIDATE_TIMEOUT + std::time::Duration::from_millis(100));
    assert!(!temporary.exists());
    assert!(!output.exists());
}

/// 每个候选都有独立时限，全部超时也最多执行四次。
#[tokio::test(start_paused = true)]
async fn all_deadlines_exhaust_with_safe_frame_error() {
    let mut calls = 0;
    let start = tokio::time::Instant::now();
    let error = try_candidates(urls(6), &active(), |_, attempt_cancel| {
        calls += 1;
        async move {
            assert!(!attempt_cancel());
            super::super::wait_for_cancel(&attempt_cancel).await;
            Err(CommandError::new("VIDEO_CANCELLED", "超时取消"))
        }
    })
    .await
    .unwrap_err();
    assert_eq!(calls, 4);
    assert_eq!(error.code, "VIDEO_FRAME_FAILED");
    assert!(start.elapsed() >= FRAME_CANDIDATE_TIMEOUT * 4);
}

/// 提交一旦开始必须等其完成；提交跨越期限仍不重复执行已成功的抽帧。
#[tokio::test(start_paused = true)]
async fn completed_commit_is_not_dropped_or_retried() {
    let mut calls = 0;
    let committed = Arc::new(AtomicBool::new(false));
    let result = try_candidates(urls(2), &active(), |_, _attempt_cancel| {
        calls += 1;
        let committed = committed.clone();
        async move {
            tokio::time::sleep(FRAME_CANDIDATE_TIMEOUT + std::time::Duration::from_millis(1)).await;
            committed.store(true, Ordering::SeqCst);
            Ok(())
        }
    })
    .await;
    assert!(result.is_ok());
    assert!(committed.load(Ordering::SeqCst));
    assert_eq!(calls, 1);
}
