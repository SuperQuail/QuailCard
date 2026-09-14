//! 真实轮询多个 future，验证有界并行、稳定顺序及取消回收。
use super::*;
use crate::services::video_tasks::VideoTaskRegistry;
use std::sync::{
    atomic::{AtomicUsize, Ordering},
    Arc,
};
use tokio::sync::Barrier;

/// 每个测试创建独立取消域，避免任务状态相互污染。
fn control() -> VideoControl {
    let record = VideoTaskRecord::new("shots-work", "key", "url");
    VideoTaskRegistry::default()
        .register("owner", &record)
        .unwrap()
}

#[tokio::test(start_paused = true)]
/// 三方屏障要求真正同时活跃；逆序延迟不能改变最终提交序号。
async fn target_parallelism_is_three_and_order_is_stable() {
    let control = control();
    let barrier = Arc::new(Barrier::new(3));
    let active = Arc::new(AtomicUsize::new(0));
    let peak = Arc::new(AtomicUsize::new(0));
    let finished = Arc::new(std::sync::Mutex::new(Vec::new()));
    let jobs = (0..9).map(|id| {
        let (barrier, active, peak, finished) = (
            barrier.clone(),
            active.clone(),
            peak.clone(),
            finished.clone(),
        );
        async move {
            let now = active.fetch_add(1, Ordering::SeqCst) + 1;
            peak.fetch_max(now, Ordering::SeqCst);
            if id < 3 {
                barrier.wait().await;
            }
            tokio::time::sleep(std::time::Duration::from_millis((9 - id) * 10)).await;
            finished.lock().unwrap().push(id);
            active.fetch_sub(1, Ordering::SeqCst);
            Ok(id)
        }
    });
    let results = collect_targets(jobs, 9, &control, |_| true).await.unwrap();
    assert_eq!(results, (0..9).collect::<Vec<_>>());
    assert_eq!(peak.load(Ordering::SeqCst), 3);
    assert_ne!(*finished.lock().unwrap(), results);
    assert!(control.snapshot().step.contains("完成 9 / 运行 0 / 等待 0"));
}

#[tokio::test(start_paused = true)]
/// 已运行的假进程必须异步完成回收，后续目标不能在取消后启动。
async fn cancellation_drains_media_before_returning() {
    let control = control();
    let started = AtomicUsize::new(0);
    let reclaimed = AtomicUsize::new(0);
    let barrier = Barrier::new(3);
    let jobs = (0..9).map(|id| {
        let (control, started, reclaimed, barrier) = (&control, &started, &reclaimed, &barrier);
        async move {
            started.fetch_add(1, Ordering::SeqCst);
            barrier.wait().await;
            if id == 0 {
                control.cancel();
                return Err(cancelled());
            }
            control.cancelled().await;
            tokio::time::sleep(std::time::Duration::from_millis(20)).await;
            reclaimed.fetch_add(1, Ordering::SeqCst);
            Ok(id)
        }
    });
    assert_eq!(
        collect_targets(jobs, 9, &control, |_| true)
            .await
            .unwrap_err()
            .code,
        "VIDEO_CANCELLED"
    );
    assert_eq!(started.load(Ordering::SeqCst), 3);
    assert_eq!(reclaimed.load(Ordering::SeqCst), 2);
}

#[tokio::test(start_paused = true)]
/// 较晚目标先完成也必须输给原序较早的同一帧，不产生补充筛选调用。
async fn earlier_target_wins_conflict_after_out_of_order_completion() {
    let control = control();
    let jobs = (0..3).map(|id| async move {
        tokio::time::sleep(std::time::Duration::from_millis((3 - id) * 10)).await;
        Ok((id, if id == 2 { 11.0 } else { 10.0 }))
    });
    let ordered = collect_targets(jobs, 3, &control, |_| true).await.unwrap();
    let mut used = HashSet::new();
    let winners: Vec<_> = ordered
        .into_iter()
        .filter(|(_, seconds)| claim_frame(&mut used, *seconds))
        .map(|(id, _)| id)
        .collect();
    assert_eq!(winners, vec![0, 2]);
}

#[tokio::test]
/// 单个目标意外失败仅缺席提交列表，后续仍执行且父任务保持未取消。
async fn target_error_skips_only_itself_and_keeps_note_successful() {
    let control = control();
    let jobs = (0..7).map(|id| async move {
        tokio::task::yield_now().await;
        if id == 1 {
            Err(CommandError::new("FAKE_TARGET", "目标失败"))
        } else {
            Ok(id)
        }
    });
    let results = collect_targets(jobs, 7, &control, |_| true).await.unwrap();
    assert_eq!(results, vec![0, 2, 3, 4, 5, 6]);
    assert!(!control.is_cancelled());
    assert!(control.snapshot().message.contains("未选择 1 个"));
}
