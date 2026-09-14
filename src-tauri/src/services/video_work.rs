//! 有界工作集：限制在途未来数量，并保证结果按输入编号返回。
//!
//! 分块压缩与同层归并共用这一份调度，避免每个分块各开一套无界并发。
use std::future::Future;

use futures_util::stream::{FuturesUnordered, StreamExt};

use crate::error::CommandError;

/// 单任务活跃工作项上限；与设计文档的并发预算一致。
pub(crate) const WORK_LIMIT: usize = 3;

/// 按并发上限推进一组工作，结果严格按输入顺序返回。
///
/// 任一项失败立即返回错误：其余在途未来被丢弃，网络请求随之取消，不继续排队。
/// 每完成一项调用一次 on_done，由调用方聚合唯一进度，工作项不直接写全局状态。
pub(crate) async fn ordered<T, F>(
    limit: usize,
    works: Vec<F>,
    on_done: &(dyn Fn() + Send + Sync),
) -> Result<Vec<T>, CommandError>
where
    F: Future<Output = Result<T, CommandError>>,
{
    let total = works.len();
    let mut slots: Vec<Option<T>> = std::iter::repeat_with(|| None).take(total).collect();
    let mut pending = works.into_iter().enumerate();
    let mut active = FuturesUnordered::new();
    for _ in 0..limit.max(1) {
        let Some((index, work)) = pending.next() else {
            break;
        };
        active.push(tagged(index, work));
    }
    while let Some((index, result)) = active.next().await {
        slots[index] = Some(result?);
        on_done();
        if let Some((index, work)) = pending.next() {
            active.push(tagged(index, work));
        }
    }
    Ok(slots.into_iter().flatten().collect())
}

/// 统一的标签包装：同名函数让两处插入产生同一未来类型，便于放进同一个无序集合。
async fn tagged<T, F>(index: usize, work: F) -> (usize, Result<T, CommandError>)
where
    F: Future<Output = Result<T, CommandError>>,
{
    (index, work.await)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    };
    use tokio::sync::{oneshot, Barrier};

    #[tokio::test]
    /// 空工作集与超过上限的工作数都不报错，结果保持编号顺序。
    async fn handles_empty_and_oversized_work_sets() {
        let empty: Result<Vec<u8>, CommandError> = ordered(
            WORK_LIMIT,
            Vec::<std::future::Ready<Result<u8, CommandError>>>::new(),
            &|| {},
        )
        .await;
        assert!(empty.unwrap().is_empty());
        let works = (0..WORK_LIMIT * 2)
            .map(|index| async move { Ok::<usize, CommandError>(index) })
            .collect();
        assert_eq!(
            ordered(WORK_LIMIT, works, &|| {}).await.unwrap(),
            (0..WORK_LIMIT * 2).collect::<Vec<_>>()
        );
    }

    #[tokio::test]
    /// 上限内的工作真正同时在途；峰值恰好等于上限，超出上限的工作必须等待。
    async fn runs_up_to_limit_concurrently_and_queues_the_rest() {
        let current = Arc::new(AtomicUsize::new(0));
        let peak = Arc::new(AtomicUsize::new(0));
        let barrier = Arc::new(Barrier::new(WORK_LIMIT));
        let mut works = Vec::new();
        for index in 0..WORK_LIMIT * 3 {
            let current = Arc::clone(&current);
            let peak = Arc::clone(&peak);
            let barrier = Arc::clone(&barrier);
            works.push(async move {
                let now = current.fetch_add(1, Ordering::SeqCst) + 1;
                peak.fetch_max(now, Ordering::SeqCst);
                if index < WORK_LIMIT {
                    // 只有首轮工作互相等待：能一起到达说明调度器确实并发启动。
                    barrier.wait().await;
                }
                tokio::task::yield_now().await;
                current.fetch_sub(1, Ordering::SeqCst);
                Ok::<usize, CommandError>(index)
            });
        }
        let result = ordered(WORK_LIMIT, works, &|| {}).await.unwrap();
        assert_eq!(result, (0..WORK_LIMIT * 3).collect::<Vec<_>>());
        assert_eq!(peak.load(Ordering::SeqCst), WORK_LIMIT);
        assert_eq!(current.load(Ordering::SeqCst), 0);
    }

    #[tokio::test]
    /// 完成顺序与输入相反时，结果仍按原编号排列。
    async fn preserves_input_order_despite_reverse_completion() {
        let mut senders = Vec::new();
        let mut works = Vec::new();
        for index in 0..WORK_LIMIT * 2 {
            let (sender, receiver) = oneshot::channel::<()>();
            senders.push(sender);
            works.push(async move {
                receiver
                    .await
                    .map_err(|_| CommandError::new("TEST", "release"))?;
                Ok::<usize, CommandError>(index)
            });
        }
        // 控制器按倒序释放；若结果按完成顺序排列，断言会失败。
        let release = async move {
            for sender in senders.into_iter().rev() {
                let _ = sender.send(());
                tokio::task::yield_now().await;
            }
        };
        let (result, ()) = tokio::join!(ordered(WORK_LIMIT, works, &|| {}), release);
        assert_eq!(result.unwrap(), (0..WORK_LIMIT * 2).collect::<Vec<_>>());
    }

    #[tokio::test]
    /// 任一项失败立即返回错误，其余在途工作被丢弃而不是继续跑完。
    async fn stops_and_drops_remaining_work_on_first_error() {
        /// 用 Drop 计数确认未完成的工作确实被丢弃。
        struct Mark(Arc<AtomicUsize>);
        impl Drop for Mark {
            fn drop(&mut self) {
                self.0.fetch_add(1, Ordering::SeqCst);
            }
        }
        /// 同一集合需要容纳不同具体类型的未来，用别名避免类型注释过长。
        type BoxedWork =
            std::pin::Pin<Box<dyn Future<Output = Result<usize, CommandError>> + Send>>;
        let dropped = Arc::new(AtomicUsize::new(0));
        let slow = Mark(Arc::clone(&dropped));
        let works: Vec<BoxedWork> = vec![
            Box::pin(async { Err(CommandError::new("TEST_FAILED", "第一项失败")) }),
            Box::pin(async move {
                let _slow = slow;
                std::future::pending::<()>().await;
                Ok(1)
            }),
            Box::pin(async { Ok(0) }),
            Box::pin(async { Ok(0) }),
        ];
        let error = ordered(WORK_LIMIT, works, &|| {}).await.unwrap_err();
        assert_eq!(error.code, "TEST_FAILED");
        assert_eq!(dropped.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    /// 上限为 0 时退化为串行，不出现除零或死锁。
    async fn zero_limit_falls_back_to_serial() {
        let current = Arc::new(AtomicUsize::new(0));
        let peak = Arc::new(AtomicUsize::new(0));
        let mut works = Vec::new();
        for index in 0..3 {
            let current = Arc::clone(&current);
            let peak = Arc::clone(&peak);
            works.push(async move {
                let now = current.fetch_add(1, Ordering::SeqCst) + 1;
                peak.fetch_max(now, Ordering::SeqCst);
                tokio::task::yield_now().await;
                current.fetch_sub(1, Ordering::SeqCst);
                Ok::<usize, CommandError>(index)
            });
        }
        assert_eq!(ordered(0, works, &|| {}).await.unwrap(), vec![0, 1, 2]);
        assert_eq!(peak.load(Ordering::SeqCst), 1);
    }
}
