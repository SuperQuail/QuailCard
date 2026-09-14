use super::agent_events::{AgentEvent, AgentEventSink};
use crate::{agent_models::AgentRun, error::CommandError};
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc, Mutex,
};
use tokio::sync::Notify;

/// 知识库任务登记与写入握手，状态锁不跨 await。
#[derive(Default)]
pub(crate) struct AgentTasks {
    current: Mutex<Option<Arc<AgentControl>>>,
}

pub(crate) struct AgentControl {
    pub owner: String,
    pub root: String,
    state: Mutex<AgentRun>,
    cancelled: AtomicBool,
    cancel_notify: Notify,
    ready: Notify,
    /// 已确认可写的操作身份；状态谓词不会被重复确认放大成下一次写入的许可。
    confirmed: Mutex<Option<String>>,
    changed: Notify,
    /// 只保留弱引用，控制器不能与树形成生命周期环。
    subagents: Mutex<Option<std::sync::Weak<super::subagents::Subagents>>>,
}

/// 待编辑器确认的写入身份；轮询路径只复制这些字段，不克隆流式正文。
pub(crate) struct PendingWrite {
    pub execution_id: String,
    pub session_id: String,
    pub path: String,
    pub operation: String,
}

impl AgentEventSink for AgentControl {
    /// 事件是唯一的状态入口；快照字段因此与循环进度保持一致。
    fn emit(&self, event: AgentEvent) {
        match event {
            AgentEvent::StepStart {
                message_id,
                reasoning_message_id,
                ..
            } => self.update(|state| {
                state.text.clear();
                state.reasoning.clear();
                state.text_message_id = message_id;
                state.reasoning_message_id = reasoning_message_id;
                state.phase = "思考与回答".into();
            }),
            AgentEvent::TextDelta { text } => self.update(|state| state.text.push_str(&text)),
            // 推理只用于实时展示，不写入会话存储。
            AgentEvent::ReasoningDelta { text } => {
                self.update(|state| state.reasoning.push_str(&text));
            }
            AgentEvent::TextCommitted { .. } => self.update(|state| state.text.clear()),
            AgentEvent::ToolStart { label, .. } => {
                self.update(|state| state.phase = label.into());
            }
            // 不设无进展硬停：阶段文案持续显示停滞，值守者据此决定是否停止。
            AgentEvent::Stalled { rounds, .. } => {
                self.update(|state| state.phase = format!("连续 {rounds} 轮无进展"));
            }
        }
    }
}

impl AgentTasks {
    /// 相同请求重试复用当前身份，活动任务不允许被另一个请求替换。
    pub(crate) fn register(
        &self,
        owner: &str,
        root: &str,
        id: &str,
        session: &str,
    ) -> Result<(Arc<AgentControl>, bool), CommandError> {
        let mut current = self
            .current
            .lock()
            .map_err(|_| CommandError::new("INTERNAL_ERROR", "Agent 状态不可用"))?;
        if let Some(control) = current.as_ref() {
            if control.owner == owner && control.root == root && control.snapshot().id == id {
                return Ok((control.clone(), false));
            }
            if control.snapshot().state == "running" {
                return Err(CommandError::new(
                    "AGENT_BUSY",
                    "Agent 正在处理任务，请先停止或等待完成",
                ));
            }
        }
        let control = Arc::new(AgentControl {
            owner: owner.into(),
            root: root.into(),
            state: Mutex::new(AgentRun {
                id: id.into(),
                session_id: session.into(),
                state: "running".into(),
                phase: "准备资料".into(),
                ..Default::default()
            }),
            cancelled: AtomicBool::new(false),
            cancel_notify: Notify::new(),
            ready: Notify::new(),
            confirmed: Mutex::new(None),
            changed: Notify::new(),
            subagents: Mutex::new(None),
        });
        *current = Some(control.clone());
        Ok((control, true))
    }

    /// 任务数据只能被发起窗口读取与继续。
    pub(crate) fn get(&self, owner: &str, id: &str) -> Result<Arc<AgentControl>, CommandError> {
        self.current
            .lock()
            .map_err(|_| CommandError::new("INTERNAL_ERROR", "Agent 状态不可用"))?
            .as_ref()
            .filter(|c| c.owner == owner && c.snapshot().id == id)
            .cloned()
            .ok_or_else(|| CommandError::new("AGENT_RUN_MISSING", "Agent 任务不存在"))
    }

    /// 观察区分已回收身份与窗口越权，不能把有效但无权的任务降级为冷历史。
    pub(crate) fn get_for_observation(
        &self,
        owner: &str,
        id: &str,
    ) -> Result<Option<Arc<AgentControl>>, CommandError> {
        let current = self
            .current
            .lock()
            .map_err(|_| CommandError::new("INTERNAL_ERROR", "Agent 状态不可用"))?;
        let Some(control) = current
            .as_ref()
            .filter(|control| control.snapshot().id == id)
        else {
            return Ok(None);
        };
        if control.owner != owner {
            return Err(CommandError::new(
                "SUBAGENT_FORBIDDEN",
                "无权读取该 Agent 任务",
            ));
        }
        Ok(Some(control.clone()))
    }

    /// 换库必须先完成取消握手，不能把任务写入新知识库。
    pub(crate) fn ensure_idle(&self) -> Result<(), CommandError> {
        if self
            .current
            .lock()
            .map_err(|_| CommandError::new("INTERNAL_ERROR", "Agent 状态不可用"))?
            .as_ref()
            .is_some_and(|c| c.snapshot().state == "running")
        {
            return Err(CommandError::new(
                "AGENT_BUSY",
                "请先停止 Agent 后再切换知识库",
            ));
        }
        Ok(())
    }
}

impl AgentControl {
    /// 树由根用例拥有，控制器弱引用不会产生生命周期环。
    pub(crate) fn attach_subagents(&self, tree: &Arc<super::subagents::Subagents>) {
        *self.subagents.lock().unwrap_or_else(|e| e.into_inner()) = Some(Arc::downgrade(tree));
    }
    /// 根收尾后弱引用失效，历史查询不能隐式恢复模型执行。
    pub(crate) fn subagents(&self) -> Option<Arc<super::subagents::Subagents>> {
        self.subagents.lock().ok()?.as_ref()?.upgrade()
    }
    /// 轮询路径只读取待写入字段，避免为协调保存克隆完整流式文本。
    pub(crate) fn pending_write(&self) -> Option<PendingWrite> {
        let state = self.state.lock().unwrap_or_else(|e| e.into_inner());
        Some(PendingWrite {
            execution_id: state.id.clone(),
            session_id: state.session_id.clone(),
            path: state.pending_write.clone()?,
            operation: state.pending_write_id.clone()?,
        })
    }
    /// 身份比较不读取正文；整树确认据此匹配真实执行身份。
    pub(crate) fn is_execution(&self, id: &str) -> bool {
        self.state.lock().unwrap_or_else(|e| e.into_inner()).id == id
    }
    /// 快照完整克隆，客户端无需依赖不可靠的事件投递。
    pub(crate) fn snapshot(&self) -> AgentRun {
        self.state.lock().unwrap_or_else(|e| e.into_inner()).clone()
    }
    /// 每次可见变化递增序号，终态不能被迟到增量覆盖。
    pub(crate) fn update(&self, action: impl FnOnce(&mut AgentRun)) {
        let mut state = self.state.lock().unwrap_or_else(|e| e.into_inner());
        if state.state != "running" {
            return;
        }
        action(&mut state);
        state.sequence += 1;
        self.changed.notify_waiters();
    }
    /// 原子读取停止标记；统一循环入口据此收尾，不参与握手清理。
    pub(crate) fn is_cancelled(&self) -> bool {
        self.cancelled.load(Ordering::SeqCst)
    }
    /// 停止唤醒网络等待与编辑器握手，已经提交的操作保留。
    pub(crate) fn cancel(&self) {
        self.cancelled.store(true, Ordering::SeqCst);
        self.cancel_notify.notify_waiters();
    }
    /// 注册通知先于读标记，防止丢失停止信号。
    pub(crate) async fn cancelled(&self) {
        let wait = self.cancel_notify.notified();
        tokio::pin!(wait);
        wait.as_mut().enable();
        if !self.cancelled.load(Ordering::SeqCst) {
            wait.await;
        }
    }
    /// 写入前要求发起窗口排空草稿，工具参数始终留在后端。
    pub(crate) async fn prepare_write(
        &self,
        path: &str,
        operation: &str,
    ) -> Result<(), CommandError> {
        // 许可只属于本次操作：先清除上一次确认，过期许可不能放行未排空草稿的写入。
        self.set_confirmed(None);
        self.update(|state| {
            state.pending_write = Some(path.into());
            state.pending_write_id = Some(operation.into());
            state.phase = "等待编辑器保存".into();
        });
        loop {
            // 先登记等待再判定状态；唤醒后必须复验本次操作是否被确认。
            let wait = self.ready.notified();
            tokio::pin!(wait);
            wait.as_mut().enable();
            if self.is_cancelled() {
                return Err(CommandError::new("AGENT_CANCELLED", "已停止"));
            }
            if self.is_confirmed(operation) {
                return Ok(());
            }
            tokio::select! { biased;
                _ = self.cancelled() => return Err(CommandError::new("AGENT_CANCELLED", "已停止")),
                _ = &mut wait => {}
            }
        }
    }
    /// 前端保持编辑器短暂只读，直到后端完成这一次写入。
    pub(crate) async fn acknowledge(&self, operation: &str) -> Result<(), CommandError> {
        if self.snapshot().pending_write_id.as_deref() != Some(operation) {
            return Err(CommandError::validation("写入握手已失效"));
        }
        // 确认是状态而不是一次性许可：既不会丢失唤醒，也不会泄漏给下一次写入。
        self.set_confirmed(Some(operation));
        self.ready.notify_one();
        loop {
            let wait = self.changed.notified();
            tokio::pin!(wait);
            wait.as_mut().enable();
            if self.snapshot().pending_write_id.as_deref() != Some(operation) {
                return Ok(());
            }
            wait.await;
        }
    }
    /// 编辑器写前握手不设总执行时限；失败或取消先清除标记，
    /// 避免已放弃的等待继续被整树观察报告为待保存写入。
    pub(crate) async fn confirm_write(
        &self,
        path: &str,
        operation: &str,
    ) -> Result<(), CommandError> {
        let result = self.prepare_write(path, operation).await;
        if result.is_err() {
            self.update(|state| {
                state.pending_write = None;
                state.pending_write_id = None;
            });
        }
        result
    }
    /// 只有当前操作被确认才放行写入；其他操作的许可不构成依据。
    fn is_confirmed(&self, operation: &str) -> bool {
        self.confirmed
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .as_deref()
            == Some(operation)
    }
    /// 写入与清除都在同一把锁内，读取方不会看到半个确认状态。
    fn set_confirmed(&self, operation: Option<&str>) {
        *self.confirmed.lock().unwrap_or_else(|e| e.into_inner()) = operation.map(str::to_owned);
    }
    /// 完成状态先移除待写入标记，确保前端只读状态总能释放。
    pub(crate) fn complete(&self, error: Option<&CommandError>) {
        let stopped = self.cancelled.load(Ordering::SeqCst);
        self.set_confirmed(None);
        self.update(|state| {
            state.pending_write = None;
            state.pending_write_id = None;
            state.state = if stopped {
                "cancelled"
            } else if error.is_some() {
                "failed"
            } else if state.waiting_reason.as_deref() == Some("waitingUser") {
                "waiting"
            } else if state.goal_phase == "blocked" {
                "blocked"
            } else if ["active", "paused"].contains(&state.goal_phase.as_str()) {
                "paused"
            } else {
                "completed"
            }
            .into();
            state.error = error.map(|e| e.message.clone());
            state.phase = if stopped { "已停止" } else { "已结束" }.into();
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    /// 事件投影成快照：文本、消息身份与阶段保持一致。
    fn events_project_into_snapshot() {
        let tasks = AgentTasks::default();
        let (control, _) = tasks.register("main", "root", "run", "session").unwrap();
        control.emit(AgentEvent::StepStart {
            step: 1,
            message_id: "m1".into(),
            reasoning_message_id: "r1".into(),
        });
        control.emit(AgentEvent::TextDelta {
            text: "你好".into(),
        });
        let snapshot = control.snapshot();
        assert_eq!(snapshot.text, "你好");
        assert_eq!(snapshot.text_message_id, "m1");
        assert_eq!(snapshot.phase, "思考与回答");
        control.emit(AgentEvent::ToolStart {
            name: "read_note".into(),
            label: "读取笔记",
        });
        assert_eq!(control.snapshot().phase, "读取笔记");
        control.emit(AgentEvent::TextCommitted {
            message_id: "m1".into(),
        });
        assert!(control.snapshot().text.is_empty());
        control.emit(AgentEvent::Stalled { step: 4, rounds: 3 });
        assert_eq!(control.snapshot().phase, "连续 3 轮无进展");
        control.cancel();
    }

    #[tokio::test(start_paused = true)]
    /// 超过旧执行时限仍等待正确确认，但取消必须释放待写入标记。
    async fn write_handshake_has_no_deadline_and_cancellation_clears_pending_marker() {
        let tasks = AgentTasks::default();
        let (control, _) = tasks.register("main", "root", "run", "session").unwrap();
        let pending = {
            let control = control.clone();
            tokio::spawn(async move { control.confirm_write("note.md", "operation").await })
        };
        tokio::task::yield_now().await;
        assert!(control.pending_write().is_some());
        tokio::time::advance(std::time::Duration::from_secs(86_401)).await;
        tokio::task::yield_now().await;
        assert!(!pending.is_finished());
        control.cancel();
        assert_eq!(pending.await.unwrap().unwrap_err().code, "AGENT_CANCELLED");
        assert!(control.pending_write().is_none());
    }

    #[tokio::test]
    /// 确认只属于本次操作：重复确认不会把许可泄漏给下一次写入。
    async fn duplicate_acknowledgement_never_releases_a_later_write() {
        let tasks = AgentTasks::default();
        let (control, _) = tasks.register("main", "root", "run", "session").unwrap();
        let first = {
            let control = control.clone();
            tokio::spawn(async move { control.prepare_write("a.md", "op-a").await })
        };
        while control.pending_write().is_none() {
            tokio::task::yield_now().await;
        }
        // 同一操作被确认两次：第二次到达时后端写入尚未完成。
        let acknowledged = {
            let control = control.clone();
            tokio::spawn(async move { control.acknowledge("op-a").await })
        };
        let duplicate = {
            let control = control.clone();
            tokio::spawn(async move { control.acknowledge("op-a").await })
        };
        assert!(first.await.unwrap().is_ok());
        control.update(|state| {
            state.pending_write = None;
            state.pending_write_id = None;
        });
        assert!(acknowledged.await.unwrap().is_ok());
        assert!(duplicate.await.unwrap().is_ok());
        // 下一次写入必须重新等待属于自己的确认，不能被上一个操作的许可放行。
        let second = {
            let control = control.clone();
            tokio::spawn(async move { control.prepare_write("b.md", "op-b").await })
        };
        while control.pending_write().is_none() {
            tokio::task::yield_now().await;
        }
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        assert!(!second.is_finished());
        control.cancel();
        assert!(second.await.unwrap().is_err());
    }
}

#[cfg(test)]
mod autonomy_tests {
    include!("agent_task_autonomy_tests.rs");
}
