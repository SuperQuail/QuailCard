//! 不依赖网络的 single-flight 契约测试。
use super::sources::*;
use super::*;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

/// 固定键用于验证同一来源只创建一个共享 future。
fn key() -> SourceKey {
    SourceKey {
        page: 1,
        cid: 7,
        quality: Some(80),
        codec: Some(7),
    }
}

/// 多等待者交错轮询时也只执行一次，并在后续调用中复用成功结果。
#[tokio::test]
async fn concurrent_success_is_cached() {
    let cache = SourceCache::default();
    let clone = cache.clone();
    let count = Arc::new(AtomicUsize::new(0));
    let cancel: CancelHandle = Arc::new(|| false);
    let resolve = || {
        let count = count.clone();
        async move {
            count.fetch_add(1, Ordering::SeqCst);
            tokio::task::yield_now().await;
            Ok(Some(FrameSource::Local(PathBuf::from("shared"))))
        }
        .boxed()
    };
    let (a, b) = tokio::join!(
        cache.get(key(), &resolve, &cancel),
        clone.get(key(), &resolve, &cancel)
    );
    assert_eq!(a.unwrap(), b.unwrap());
    assert!(cache.get(key(), &resolve, &cancel).await.unwrap().is_some());
    assert_eq!(count.load(Ordering::SeqCst), 1);
    let other = SourceKey { cid: 8, ..key() };
    cache.get(other, &resolve, &cancel).await.unwrap();
    assert_eq!(count.load(Ordering::SeqCst), 2);
}

/// 失败唤醒全部等待者，两个目标不能各自耗费两轮独立重试预算。
#[tokio::test]
async fn failed_flights_have_one_bounded_retry() {
    let cache = SourceCache::default();
    let count = Arc::new(AtomicUsize::new(0));
    let cancel: CancelHandle = Arc::new(|| false);
    let resolve = || {
        let count = count.clone();
        async move {
            count.fetch_add(1, Ordering::SeqCst);
            tokio::task::yield_now().await;
            Err(CommandError::new("VIDEO_SOURCE_FAILED", "来源失败"))
        }
        .boxed()
    };
    let (a, b) = tokio::join!(
        cache.get(key(), &resolve, &cancel),
        cache.get(key(), &resolve, &cancel)
    );
    assert!(a.is_err() && b.is_err());
    assert!(cache.get(key(), &resolve, &cancel).await.is_err());
    assert_eq!(count.load(Ordering::SeqCst), 2);
}

/// 首次失败后成功也只执行两次，所有等待者及后续目标共享成功结果。
#[tokio::test]
async fn retry_success_is_shared_and_cached() {
    let cache = SourceCache::default();
    let count = Arc::new(AtomicUsize::new(0));
    let cancel: CancelHandle = Arc::new(|| false);
    let resolve = || {
        let count = count.clone();
        async move {
            let attempt = count.fetch_add(1, Ordering::SeqCst);
            tokio::task::yield_now().await;
            if attempt == 0 {
                Err(CommandError::new("VIDEO_SOURCE_FAILED", "来源失败"))
            } else {
                Ok(Some(FrameSource::Local(PathBuf::from("retry"))))
            }
        }
        .boxed()
    };
    let (a, b) = tokio::join!(
        cache.get(key(), &resolve, &cancel),
        cache.get(key(), &resolve, &cancel)
    );
    assert_eq!(a.unwrap(), b.unwrap());
    assert!(cache.get(key(), &resolve, &cancel).await.unwrap().is_some());
    assert_eq!(count.load(Ordering::SeqCst), 2);
}

/// 父取消后清空缓存中的未完成 future，销毁来源资源而不遗留挂起下载。
#[tokio::test]
async fn cancellation_reclaims_cached_flight() {
    struct Guard(Arc<AtomicBool>);
    impl Drop for Guard {
        /// 作为网络半文件资源的替身，确认 future 最终被回收。
        fn drop(&mut self) {
            self.0.store(true, Ordering::SeqCst);
        }
    }
    let cache = SourceCache::default();
    let flag = Arc::new(AtomicBool::new(false));
    let dropped = Arc::new(AtomicBool::new(false));
    let cancel: CancelHandle = {
        let flag = flag.clone();
        Arc::new(move || flag.load(Ordering::SeqCst))
    };
    let resolve = || {
        let guard = Guard(dropped.clone());
        async move {
            let _guard = guard;
            futures_util::future::pending::<Result<Option<FrameSource>, CommandError>>().await
        }
        .boxed()
    };
    let operation = cache.get(key(), &resolve, &cancel);
    tokio::pin!(operation);
    assert!(futures_util::poll!(&mut operation).is_pending());
    flag.store(true, Ordering::SeqCst);
    assert_eq!(operation.await.unwrap_err().code, "VIDEO_CANCELLED");
    assert!(dropped.load(Ordering::SeqCst));
}
