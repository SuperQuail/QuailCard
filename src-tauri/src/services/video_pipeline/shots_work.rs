//! 配图目标专用的有界收集器：先排稳定序号，失败也等待已启动媒体回收。
use super::*;
use futures_util::{stream::FuturesUnordered, StreamExt};
use std::future::Future;

/// 只推进三个目标，不 spawn；结果排序不依赖网络完成先后。
pub(super) async fn collect_targets<T, F>(
    jobs: impl Iterator<Item = F>,
    total: usize,
    control: &VideoControl,
    is_selected: impl Fn(&T) -> bool,
) -> Result<Vec<T>, CommandError>
where
    F: Future<Output = Result<T, CommandError>>,
{
    let mut pending = jobs
        .enumerate()
        .map(|(index, job)| async move { (index, job.await) });
    let mut active = FuturesUnordered::new();
    let mut results = Vec::with_capacity(total);
    let mut failure = None;
    let mut started = 0;
    let mut done = 0;
    let mut selected = 0;
    if control.is_cancelled() {
        return Err(cancelled());
    }
    for job in pending.by_ref().take(3) {
        active.push(job);
        started += 1;
    }
    progress(control, done, active.len(), total - started, selected);
    while let Some((index, result)) = active.next().await {
        done += 1;
        match result {
            Ok(value) => {
                selected += usize::from(is_selected(&value));
                results.push((index, value));
            }
            Err(error) if error.code == "VIDEO_CANCELLED" => {
                if failure.is_none() {
                    failure = Some(error);
                }
                control.cancel();
            }
            Err(error) => {
                // 单目标异常不改变父任务终态；它的标记由最终清理移除。
                eprintln!("VIDEO_SHOT_TARGET_SKIPPED(detail): {error}");
            }
        }
        // 不用 try_collect 或取消 select：它们会丢掉仍在等待真实进程退出的 future。
        if failure.is_none() && !control.is_cancelled() {
            if let Some(job) = pending.next() {
                active.push(job);
                started += 1;
            }
        }
        progress(control, done, active.len(), total - started, selected);
    }
    if control.is_cancelled() {
        control.update(|status| {
            status.message = "取消后已启动 ffmpeg 全部回收；未提交候选不会保存附件".into()
        });
    }
    if let Some(error) = failure {
        return Err(error);
    }
    if control.is_cancelled() {
        return Err(cancelled());
    }
    results.sort_by_key(|(index, _)| *index);
    Ok(results.into_iter().map(|(_, value)| value).collect())
}

/// 只有协调器写聚合状态，工作项不得相互覆盖当前目标描述。
fn progress(control: &VideoControl, done: usize, running: usize, waiting: usize, selected: usize) {
    control.update(|status| {
        status.step = format!("筛选配图：完成 {done} / 运行 {running} / 等待 {waiting}");
        status.message = format!(
            "候选已确认 {selected} 个目标，未选择 {} 个；按原目标顺序去重后保存",
            done - selected
        );
    });
}

#[cfg(test)]
#[path = "shots_work_tests.rs"]
mod tests;
