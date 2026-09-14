//! 笔记写入的按路径锁：同一路径串行、不同路径并行。
//!
//! 加锁顺序契约（避免死锁，改动前先读）：
//! 1. 只保护单个笔记文件及其 change 日志，绝不跨 await 持有；
//! 2. 绝不在持有路径锁时获取会话锁 SESSION_WRITE，也绝不在持有会话锁时获取路径锁：
//!    两把锁各自独占一段同步写盘，互不嵌套；
//! 3. 锁键必须是 vaultfs 净化后的规范相对路径，否则别名写法会落到两把锁上。
//!
//! 锁表用弱引用登记：每次取用清理已释放条目，键数量与当前并发路径数同阶，不随历史无界增长。

use crate::error::CommandError;
use std::sync::{Arc, Mutex, MutexGuard, Weak};

static PATH_LOCKS: Mutex<Vec<PathLock>> = Mutex::new(Vec::new());

/// 锁表条目；Weak 让写入结束后的条目被下一次取用回收。
struct PathLock {
    path: String,
    lock: Weak<Mutex<()>>,
}

/// 取得某条规范路径的写入锁；同一路径串行，不同路径互不阻塞。
pub(crate) fn lock(path: &str) -> Result<Arc<Mutex<()>>, CommandError> {
    let mut table = PATH_LOCKS
        .lock()
        .map_err(|_| CommandError::new("INTERNAL_ERROR", "笔记写入锁不可用"))?;
    // 每次取用都清理已释放条目，锁表上界即当前并发写入路径数。
    table.retain(|entry| entry.lock.strong_count() > 0);
    if let Some(existing) = table.iter().find(|entry| entry.path == path) {
        if let Some(lock) = existing.lock.upgrade() {
            return Ok(lock);
        }
    }
    let lock = Arc::new(Mutex::new(()));
    table.push(PathLock {
        path: path.to_owned(),
        lock: Arc::downgrade(&lock),
    });
    Ok(lock)
}

/// 等待临界区；被守卫的数据是 `()`，中毒不破坏内容，直接沿用内部值。
///
/// 调用方必须把 Arc 绑在一个比守卫更早声明的局部变量上，保证先释放守卫再释放锁对象。
pub(super) fn enter(lock: &Mutex<()>) -> MutexGuard<'_, ()> {
    lock.lock().unwrap_or_else(|error| error.into_inner())
}
