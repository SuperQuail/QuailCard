//! 视频任务注册表：进度、取消与终态保留。
#[cfg(test)]
#[path = "video_task_tests.rs"]
mod tests;

use std::{
    collections::HashMap,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex,
    },
    time::{Duration, Instant},
};

use tokio::sync::Notify;

use crate::{
    error::CommandError,
    storage::video::{TaskOrigin, VideoTaskRecord},
    video::models::VideoTaskStatus,
};

/// 终态任务的保留时长。
const TERMINAL_RETENTION: Duration = Duration::from_secs(15 * 60);

/// 终态保留秒数：磁盘上的 Agent 任务清理必须与内存保留使用同一窗口。
pub(crate) fn terminal_retention_secs() -> i64 {
    TERMINAL_RETENTION.as_secs() as i64
}

/// 任务内部状态。
struct TaskState {
    status: VideoTaskStatus,
    completed_at: Option<Instant>,
}

/// 共享任务控制块。
struct TaskControl {
    cancelled: AtomicBool,
    notify: Notify,
    /// 发起来源随任务传递，流水线重建缺失记录时不能丢失。
    origin: TaskOrigin,
    state: Mutex<TaskState>,
}

/// 面向流水线的控制句柄；克隆共享同一状态。
#[derive(Clone)]
pub(crate) struct VideoControl {
    inner: Arc<TaskControl>,
}

impl VideoControl {
    /// 创建处于运行中的任务控制块。
    fn new(task_id: String, origin: TaskOrigin) -> Self {
        Self {
            inner: Arc::new(TaskControl {
                cancelled: AtomicBool::new(false),
                notify: Notify::new(),
                origin,
                state: Mutex::new(TaskState {
                    status: VideoTaskStatus {
                        task_id,
                        state: "running".to_string(),
                        step: "准备中".to_string(),
                        message: "正在准备视频任务".to_string(),
                        ..Default::default()
                    },
                    completed_at: None,
                }),
            }),
        }
    }

    /// 更新进度、步骤或消息；终态不可被迟到进度覆盖。
    pub(crate) fn update(&self, apply: impl FnOnce(&mut VideoTaskStatus)) {
        let mut state = self
            .inner
            .state
            .lock()
            .unwrap_or_else(|poison| poison.into_inner());
        if state.completed_at.is_some() {
            return;
        }
        let previous = state.status.progress;
        let old_step = state.status.step.clone();
        let old_message = state.status.message.clone();
        apply(&mut state.status);
        if state.status.step != old_step || state.status.message != old_message {
            let line = if state.status.message != old_message {
                state.status.message.clone()
            } else {
                state.status.step.clone()
            };
            if !line.is_empty() {
                state.status.logs.push(line);
            }
            let excess = state.status.logs.len().saturating_sub(100);
            state.status.logs.drain(..excess);
        }
        state.status.progress = state.status.progress.max(previous).min(100);
        state.status.sequence = state.status.sequence.saturating_add(1);
    }

    /// 读取当前快照。
    pub(crate) fn snapshot(&self) -> VideoTaskStatus {
        self.inner
            .state
            .lock()
            .unwrap_or_else(|poison| poison.into_inner())
            .status
            .clone()
    }

    /// 任务发起来源；流水线兜底重建记录时必须沿用同一来源。
    pub(crate) fn origin(&self) -> TaskOrigin {
        self.inner.origin
    }

    /// 请求取消；下载与子进程轮询会立即感知。
    pub(crate) fn cancel(&self) {
        self.inner.cancelled.store(true, Ordering::SeqCst);
        self.inner.notify.notify_waiters();
    }

    /// 取消标记，供媒体端口与子进程使用。
    pub(crate) fn is_cancelled(&self) -> bool {
        self.inner.cancelled.load(Ordering::SeqCst)
    }

    /// 等待取消，先注册通知再检查标记，避免丢失并发唤醒。
    pub(crate) async fn cancelled(&self) {
        loop {
            let notified = self.inner.notify.notified();
            tokio::pin!(notified);
            notified.as_mut().enable();
            if self.is_cancelled() {
                return;
            }
            notified.await;
        }
    }

    /// 终态判定与完成后是否已过期。
    fn expired(&self) -> bool {
        let state = self
            .inner
            .state
            .lock()
            .unwrap_or_else(|poison| poison.into_inner());
        state
            .completed_at
            .is_some_and(|finished| finished.elapsed() >= TERMINAL_RETENTION)
    }

    /// 写终态：成功、取消与失败只写一次。
    pub(crate) fn complete(&self, result: &Result<(), CommandError>) {
        let mut state = self
            .inner
            .state
            .lock()
            .unwrap_or_else(|poison| poison.into_inner());
        if state.completed_at.is_some() {
            return;
        }
        match result {
            Ok(()) => {
                state.status.state = "completed".to_string();
                state.status.progress = 100;
                state.status.step = "已完成".to_string();
            }
            Err(error) if error.code == "VIDEO_CANCELLED" => {
                state.status.state = "cancelled".to_string();
                state.status.message = error.message.clone();
            }
            Err(error) => {
                state.status.state = "failed".to_string();
                state.status.error = Some(error.message.clone());
            }
        }
        state.status.sequence = state.status.sequence.saturating_add(1);
        state.completed_at = Some(Instant::now());
    }
}

/// 注册表条目：记录归属窗口，避免跨窗口读取任务。
struct OwnedTask {
    owner: String,
    control: VideoControl,
}

/// 单窗口单任务的注册表。
#[derive(Default)]
pub(crate) struct VideoTaskRegistry {
    tasks: Mutex<HashMap<String, OwnedTask>>,
}

impl VideoTaskRegistry {
    /**
     * 登记新任务；同一窗口已有运行中任务时拒绝。
     *
     * 装配完成后调用 register_persisted 统一处理登记与保存，避免失败占用运行槽。
     */
    pub(crate) fn register(
        &self,
        owner: &str,
        record: &VideoTaskRecord,
    ) -> Result<VideoControl, CommandError> {
        let mut tasks = self
            .tasks
            .lock()
            .unwrap_or_else(|poison| poison.into_inner());
        tasks.retain(|_, task| !task.control.expired());
        if tasks
            .values()
            .any(|task| task.owner == owner && task.control.snapshot().state == "running")
        {
            return Err(CommandError::new(
                "VIDEO_TASK_RUNNING",
                "当前窗口已有视频任务，请先停止或等待完成",
            ));
        }
        let control = VideoControl::new(record.task_id.clone(), record.origin);
        tasks.insert(
            record.task_id.clone(),
            OwnedTask {
                owner: owner.to_string(),
                control: control.clone(),
            },
        );
        Ok(control)
    }

    /// 装配完成后登记并保存，保存失败立即释放运行槽，允许用户重试。
    pub(crate) fn register_persisted(
        &self,
        owner: &str,
        record: &VideoTaskRecord,
        save: impl FnOnce() -> Result<(), CommandError>,
    ) -> Result<VideoControl, CommandError> {
        let control = self.register(owner, record)?;
        if let Err(error) = save() {
            control.complete(&Err(CommandError::new(error.code, error.message.clone())));
            return Err(error);
        }
        Ok(control)
    }

    /// 查询任务快照；非本窗口或已过期一律拒绝。
    pub(crate) fn status(
        &self,
        owner: &str,
        task_id: &str,
    ) -> Result<VideoTaskStatus, CommandError> {
        let mut tasks = self
            .tasks
            .lock()
            .unwrap_or_else(|poison| poison.into_inner());
        tasks.retain(|_, task| !task.control.expired());
        tasks
            .get(task_id)
            .filter(|task| task.owner == owner)
            .map(|task| task.control.snapshot())
            .ok_or_else(|| CommandError::new("VIDEO_TASK_NOT_FOUND", "视频任务不存在或已结束"))
    }

    /// Agent 只观察自身仍在运行的任务，不返回旧终态或其他会话资料。
    pub(crate) fn running_status(&self, owner: &str) -> Option<VideoTaskStatus> {
        let tasks = self
            .tasks
            .lock()
            .unwrap_or_else(|poison| poison.into_inner());
        tasks
            .values()
            .filter(|task| task.owner == owner)
            .map(|task| task.control.snapshot())
            .find(|status| status.state == "running")
    }

    /// 历史列表只读取跨窗口运行态，不能借此获得转录、进度详情或取消权限。
    pub(crate) fn live_state_for_history(&self, task_id: &str) -> Option<String> {
        let tasks = self
            .tasks
            .lock()
            .unwrap_or_else(|poison| poison.into_inner());
        tasks
            .get(task_id)
            .filter(|task| !task.control.expired())
            .map(|task| task.control.snapshot().state)
    }

    /// 幂等取消；未知任务视为已完成。
    pub(crate) fn cancel(&self, owner: &str, task_id: &str) -> Result<(), CommandError> {
        let tasks = self
            .tasks
            .lock()
            .unwrap_or_else(|poison| poison.into_inner());
        if let Some(task) = tasks.get(task_id).filter(|task| task.owner == owner) {
            task.control.cancel();
        }
        Ok(())
    }
}
#[path = "video_downloads.rs"]
mod downloads;
pub(crate) use downloads::VideoDownloads;
