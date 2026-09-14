use std::{
    collections::HashMap,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex,
    },
    time::{Duration, Instant},
};

use tokio::sync::Notify;
use uuid::Uuid;

use crate::{
    error::CommandError,
    models::{GenerationResult, GenerationTaskStatus},
};

const TERMINAL_RETENTION: Duration = Duration::from_secs(15 * 60);

struct TaskState {
    status: GenerationTaskStatus,
    completed_at: Option<Instant>,
}

struct TaskControl {
    cancelled: AtomicBool,
    notify: Notify,
    state: Mutex<TaskState>,
}

/// 取消令牌与状态共享同一生命周期，停止不会删除已校验草稿。
#[derive(Clone)]
pub(crate) struct GenerationControl {
    inner: Arc<TaskControl>,
}

impl GenerationControl {
    /// 先创建可查询状态，再允许调度后台工作。
    pub(crate) fn new(task_id: String) -> Self {
        Self {
            inner: Arc::new(TaskControl {
                cancelled: AtomicBool::new(false),
                notify: Notify::new(),
                state: Mutex::new(TaskState {
                    status: GenerationTaskStatus {
                        task_id,
                        state: "running".to_string(),
                        phase: "preparing".to_string(),
                        generated_count: 0,
                        result: None,
                        error: None,
                    },
                    completed_at: None,
                }),
            }),
        }
    }

    /// 更新真实执行阶段；终态不可被迟到进度覆盖。
    pub(super) fn progress(&self, phase: &str, generated: usize) {
        let mut state = self
            .inner
            .state
            .lock()
            .unwrap_or_else(|poison| poison.into_inner());
        if state.completed_at.is_none() {
            state.status.phase = phase.to_string();
            state.status.generated_count = generated;
        }
    }

    /// 同步标记停止后唤醒正在等待模型或词典的执行器。
    pub(crate) fn cancel(&self) {
        self.inner.cancelled.store(true, Ordering::SeqCst);
        self.inner.notify.notify_one();
    }

    /// 原子读取支持立即取消和调用间隙的取消。
    pub(super) fn is_cancelled(&self) -> bool {
        self.inner.cancelled.load(Ordering::SeqCst)
    }

    /// 通知保留许可，避免检查标记与开始等待之间丢失停止请求。
    pub(super) async fn cancelled(&self) {
        loop {
            let notified = self.inner.notify.notified();
            if self.is_cancelled() {
                return;
            }
            notified.await;
        }
    }

    /// 完成只写一次终态；失败保留安全错误，部分成功保留草稿。
    pub(crate) fn complete(&self, result: Result<GenerationResult, CommandError>) {
        let mut state = self
            .inner
            .state
            .lock()
            .unwrap_or_else(|poison| poison.into_inner());
        if state.completed_at.is_some() {
            return;
        }
        match result {
            Ok(result) => {
                state.status.state = if self.is_cancelled() {
                    "cancelled"
                } else {
                    "completed"
                }
                .to_string();
                state.status.generated_count = result.cards.len();
                state.status.result = Some(result);
            }
            Err(error) => {
                state.status.state = "failed".to_string();
                state.status.error = Some(error);
            }
        }
        state.completed_at = Some(Instant::now());
    }

    /// 当前阶段文案；供 Agent 侧把拆卡进度接进自己的快照，不改变任务状态。
    pub(super) fn phase(&self) -> String {
        self.status().phase
    }

    /// 克隆只包含前端允许接收的任务元数据和终态结果。
    fn status(&self) -> GenerationTaskStatus {
        self.inner
            .state
            .lock()
            .unwrap_or_else(|poison| poison.into_inner())
            .status
            .clone()
    }

    /// 运行中任务永不清除，终态按保留期自动回收。
    fn expired(&self) -> bool {
        self.inner
            .state
            .lock()
            .unwrap_or_else(|poison| poison.into_inner())
            .completed_at
            .is_some_and(|finished| finished.elapsed() >= TERMINAL_RETENTION)
    }
}

struct OwnedTask {
    owner: String,
    control: GenerationControl,
}

/// 每个窗口最多一个运行任务；查询和停止均验证窗口所有权。
#[derive(Default)]
pub(crate) struct GenerationTaskRegistry {
    tasks: Mutex<HashMap<String, OwnedTask>>,
}

impl GenerationTaskRegistry {
    /// 登记完成后才返回任务 ID，启动后立即停止也不会查无此任务。
    pub(crate) fn register(
        &self,
        owner: &str,
    ) -> Result<(String, GenerationControl), CommandError> {
        let mut tasks = self
            .tasks
            .lock()
            .unwrap_or_else(|poison| poison.into_inner());
        tasks.retain(|_, task| !task.control.expired());
        if tasks
            .values()
            .any(|task| task.owner == owner && task.control.status().state == "running")
        {
            return Err(CommandError::new(
                "GENERATION_ALREADY_RUNNING",
                "当前窗口已有拆卡任务，请先停止或等待完成",
            ));
        }
        let task_id = Uuid::now_v7().to_string();
        let control = GenerationControl::new(task_id.clone());
        tasks.insert(
            task_id.clone(),
            OwnedTask {
                owner: owner.to_string(),
                control: control.clone(),
            },
        );
        Ok((task_id, control))
    }

    /// 清理过期终态后验证所有权，避免其他窗口读到学习材料。
    fn control(&self, owner: &str, task_id: &str) -> Result<GenerationControl, CommandError> {
        let mut tasks = self
            .tasks
            .lock()
            .unwrap_or_else(|poison| poison.into_inner());
        tasks.retain(|_, task| !task.control.expired());
        tasks
            .get(task_id)
            .filter(|task| task.owner == owner)
            .map(|task| task.control.clone())
            .ok_or_else(|| CommandError::new("GENERATION_TASK_NOT_FOUND", "拆卡任务不存在或已过期"))
    }

    /// 状态查询不改变运行进度或草稿。
    pub(crate) fn status(
        &self,
        owner: &str,
        task_id: &str,
    ) -> Result<GenerationTaskStatus, CommandError> {
        Ok(self.control(owner, task_id)?.status())
    }

    /// 重复停止为幂等操作，已完成任务继续保留原有结果。
    pub(crate) fn cancel(&self, owner: &str, task_id: &str) -> Result<(), CommandError> {
        let control = self.control(owner, task_id)?;
        if control.status().state == "running" {
            control.cancel();
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    /// 立即取消、重复启动和窗口隔离均在真实注册表验证。
    async fn registration_precedes_cancel_and_preserves_owner() {
        let registry = GenerationTaskRegistry::default();
        let (id, control) = registry.register("main").unwrap();
        assert!(registry.register("main").is_err());
        assert!(registry.status("other", &id).is_err());
        registry.cancel("main", &id).unwrap();
        tokio::time::timeout(Duration::from_millis(100), control.cancelled())
            .await
            .unwrap();
        control.complete(Ok(GenerationResult {
            cards: vec![],
            warnings: vec![],
        }));
        assert_eq!(registry.status("main", &id).unwrap().state, "cancelled");
        registry.cancel("main", &id).unwrap();
        assert!(registry.register("main").is_ok());
    }

    #[test]
    /// 超过十五分钟的终态回收后不会继续暴露草稿。
    fn expires_terminal_tasks() {
        let registry = GenerationTaskRegistry::default();
        let (id, control) = registry.register("main").unwrap();
        control.complete(Ok(GenerationResult {
            cards: vec![],
            warnings: vec![],
        }));
        control.inner.state.lock().unwrap().completed_at =
            Some(Instant::now() - TERMINAL_RETENTION);
        assert_eq!(
            registry.status("main", &id).unwrap_err().code,
            "GENERATION_TASK_NOT_FOUND"
        );
    }
}
