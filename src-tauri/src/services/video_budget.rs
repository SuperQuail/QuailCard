//! 全部视频任务共享资源额度；组合根创建一次，克隆只共享 Arc。
#[path = "video_budget_model.rs"]
mod model;
pub(crate) use model::BudgetedModel;
#[cfg(test)]
#[path = "video_budget_tests.rs"]
mod tests;

use super::video_tasks::VideoControl;
use crate::error::CommandError;
use std::{
    collections::{HashMap, VecDeque},
    sync::{Arc, Mutex, MutexGuard},
    time::Duration,
};
use tokio::{
    sync::{Notify, OwnedSemaphorePermit, Semaphore},
    time::Instant,
};

#[derive(Clone)]
pub(crate) struct VideoBudget {
    inner: Arc<Shared>,
}

struct Shared {
    state: Mutex<ModelState>,
    changed: Notify,
    frame: Arc<Semaphore>,
    download: Arc<Semaphore>,
    images: Arc<Semaphore>,
}

#[derive(Default)]
struct ModelState {
    active: usize,
    providers: HashMap<String, Provider>,
    waiting: VecDeque<Waiting>,
    tasks: VecDeque<String>,
    next_id: u64,
}

#[derive(Default)]
struct Provider {
    active: usize,
    until: Option<Instant>,
}

struct Waiting {
    id: u64,
    task: String,
    provider: String,
}

/// 模型整个响应流持有此许可；失败、取消及 future 被丢弃均归还计数。
pub(crate) struct ModelPermit {
    inner: Arc<Shared>,
    provider: String,
}

/// 排队本身也由 RAII 管理，避免被丢弃的 future 堵塞公平队列。
struct QueueGuard {
    inner: Arc<Shared>,
    id: u64,
}

impl Default for VideoBudget {
    /// 限额归组合根生命周期所有，不在每个视频任务入口重新创建。
    fn default() -> Self {
        Self {
            inner: Arc::new(Shared {
                state: Mutex::new(ModelState::default()),
                changed: Notify::new(),
                frame: Arc::new(Semaphore::new(2)),
                download: Arc::new(Semaphore::new(1)),
                images: Arc::new(Semaphore::new(3)),
            }),
        }
    }
}

impl Shared {
    /// 临界区只保护队列与计数，不允许在锁内等待网络或定时器。
    fn state(&self) -> MutexGuard<'_, ModelState> {
        self.state
            .lock()
            .unwrap_or_else(|poison| poison.into_inner())
    }
}

impl ModelState {
    /// 删除已完成或取消的候选；没有待调度项的任务不继续占轮转位置。
    fn remove(&mut self, id: u64) {
        self.waiting.retain(|item| item.id != id);
        self.tasks
            .retain(|task| self.waiting.iter().any(|item| &item.task == task));
    }

    /// 按任务轮转选择就绪候选；冷却和供应商满额均不占全局槽。
    fn next(&self, now: Instant) -> Option<u64> {
        if self.active >= 3 {
            return None;
        }
        for task in &self.tasks {
            if let Some(item) = self.waiting.iter().find(|item| {
                &item.task == task
                    && self.providers.get(&item.provider).is_none_or(|provider| {
                        provider.active < 3 && provider.until.is_none_or(|until| until <= now)
                    })
            }) {
                return Some(item.id);
            }
        }
        None
    }

    /// 只等待仍在未来的冷却期限，避免已过期时间导致自旋。
    fn deadline(&self, now: Instant) -> Option<Instant> {
        self.waiting
            .iter()
            .filter_map(|item| self.providers.get(&item.provider)?.until)
            .filter(|until| *until > now)
            .min()
    }
}

impl VideoBudget {
    /// 原子检查全局与供应商额度；同任务 FIFO、就绪任务轮转，等待可取消。
    pub(crate) async fn model(
        &self,
        provider_id: &str,
        task_id: &str,
        control: &VideoControl,
    ) -> Result<ModelPermit, CommandError> {
        if control.is_cancelled() {
            return Err(cancelled());
        }
        let queue = {
            let mut state = self.inner.state();
            let id = state.next_id;
            state.next_id += 1;
            if !state.tasks.iter().any(|task| task == task_id) {
                state.tasks.push_back(task_id.to_string());
            }
            state.waiting.push_back(Waiting {
                id,
                task: task_id.to_string(),
                provider: provider_id.to_string(),
            });
            QueueGuard {
                inner: self.inner.clone(),
                id,
            }
        };
        self.inner.changed.notify_waiters();
        loop {
            let changed = self.inner.changed.notified();
            tokio::pin!(changed);
            changed.as_mut().enable();
            if control.is_cancelled() {
                return Err(cancelled());
            }
            let (permit, deadline) = self.try_model(queue.id, provider_id, task_id);
            if let Some(permit) = permit {
                self.inner.changed.notify_waiters();
                return Ok(permit);
            }
            tokio::select! {
                biased;
                _ = control.cancelled() => return Err(cancelled()),
                _ = &mut changed => {},
                _ = wait_deadline(deadline) => {},
            }
        }
    }

    /// 每次派发后将该任务排到队尾；保留其它任务的相对顺序。
    fn try_model(
        &self,
        id: u64,
        provider: &str,
        task: &str,
    ) -> (Option<ModelPermit>, Option<Instant>) {
        let mut state = self.inner.state();
        let now = Instant::now();
        if state.next(now) != Some(id) {
            return (None, state.deadline(now));
        }
        state.remove(id);
        if let Some(index) = state.tasks.iter().position(|candidate| candidate == task) {
            let task = state.tasks.remove(index).expect("轮转任务存在");
            state.tasks.push_back(task);
        }
        state.active += 1;
        state
            .providers
            .entry(provider.to_string())
            .or_default()
            .active += 1;
        (
            Some(ModelPermit {
                inner: self.inner.clone(),
                provider: provider.to_string(),
            }),
            None,
        )
    }

    /// 所有同配置任务共享且只延长冷却；调用者必须在释放本次许可前发布。
    fn cool_down(&self, provider_id: &str, delay: Duration) {
        let mut state = self.inner.state();
        let provider = state.providers.entry(provider_id.to_string()).or_default();
        let until = Instant::now() + delay;
        provider.until = Some(provider.until.map_or(until, |previous| previous.max(until)));
        drop(state);
        self.inner.changed.notify_waiters();
    }

    /// 单个抽帧阶段取得额度，不得在下载阶段提前持有。
    pub(crate) async fn frame(
        &self,
        control: &VideoControl,
    ) -> Result<OwnedSemaphorePermit, CommandError> {
        acquire(&self.inner.frame, control).await
    }

    /// 下载只允许一个在途整轨传输，释放由许可 Drop 保证。
    pub(crate) async fn download(
        &self,
        control: &VideoControl,
    ) -> Result<OwnedSemaphorePermit, CommandError> {
        acquire(&self.inner.download, control).await
    }

    /// 图片组先取得额度再加载字节，整个候选组生命周期持有。
    pub(crate) async fn images(
        &self,
        control: &VideoControl,
    ) -> Result<OwnedSemaphorePermit, CommandError> {
        acquire(&self.inner.images, control).await
    }
}

impl Drop for ModelPermit {
    /// 回收全局和供应商计数后唤醒所有可竞争的任务。
    fn drop(&mut self) {
        let mut state = self.inner.state();
        state.active -= 1;
        state
            .providers
            .get_mut(&self.provider)
            .expect("许可的供应商存在")
            .active -= 1;
        drop(state);
        self.inner.changed.notify_waiters();
    }
}

impl Drop for QueueGuard {
    /// 无论正常获准、取消还是直接 Drop，都移除排队身份。
    fn drop(&mut self) {
        self.inner.state().remove(self.id);
        self.inner.changed.notify_waiters();
    }
}

/// 没有冷却时只靠通知驱动，不创建周期性轮询。
async fn wait_deadline(deadline: Option<Instant>) {
    match deadline {
        Some(deadline) => tokio::time::sleep_until(deadline).await,
        None => std::future::pending().await,
    }
}

/// Tokio 信号量保持 FIFO；取消优先且丢弃 future 自动撤销排队。
async fn acquire(
    semaphore: &Arc<Semaphore>,
    control: &VideoControl,
) -> Result<OwnedSemaphorePermit, CommandError> {
    tokio::select! {
        biased;
        _ = control.cancelled() => Err(cancelled()),
        permit = semaphore.clone().acquire_owned() => permit.map_err(|_| cancelled()),
    }
}

/// 统一取消码，避免调用方误当作供应商故障重试。
fn cancelled() -> CommandError {
    CommandError::new("VIDEO_CANCELLED", "视频任务已取消")
}
