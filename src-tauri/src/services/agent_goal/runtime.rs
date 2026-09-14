use super::{
    check_cas, next_revision, require_active, require_user, text, validate_goal, Authority,
    DomainError,
};
use crate::agent_autonomy_models::{Goal, GoalPhase};

/// 每次准入都重新从宿主读取；默认全部不就绪，不能凭模型声称放行。
#[derive(Clone, Debug, Default)]
pub struct Admission {
    pub normal_turn_end: bool,
    pub human_pending: bool,
    pub execution_busy: bool,
    pub waiting_children: bool,
    pub persisted: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StopReason {
    User,
    Storage,
    AbnormalTurn,
}

/// 预约只绑定快照，不消耗轮数；私有序号阻止迟到通知匹配新预约。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Reservation {
    pub goal_id: String,
    pub goal_revision: u64,
    pub execution_id: String,
    sequence: u64,
}

/// 只活在内存，恢复与 Fork 必须 new；宿主可克隆候选状态，持久化成功后再发布。
#[derive(Clone, Debug)]
pub struct GoalRuntime {
    execution_id: String,
    goal_id: Option<String>,
    armed: bool,
    waiting_user: bool,
    stopped: Option<StopReason>,
    sequence: u64,
    reserved: Option<Reservation>,
    running: Option<Reservation>,
}

impl GoalRuntime {
    /// 每个根执行使用新身份，旧持久化 Goal 本身不授予续轮许可。
    pub fn new(execution_id: &str) -> Result<Self, DomainError> {
        text(execution_id, 128)?;
        Ok(Self {
            execution_id: execution_id.into(),
            goal_id: None,
            armed: false,
            waiting_user: false,
            stopped: None,
            sequence: 0,
            reserved: None,
            running: None,
        })
    }

    /// 只有直接人类创建或继续后才可武装；用户等待也只由此入口解除。
    pub fn arm(&mut self, goal: &Goal, authority: Authority) -> Result<(), DomainError> {
        require_user(authority)?;
        validate_goal(goal)?;
        require_active(goal)?;
        if self.running.is_some() {
            return Err(DomainError::Busy);
        }
        let sequence = next_revision(self.sequence)?;
        self.sequence = sequence;
        self.goal_id = Some(goal.id.clone());
        self.armed = true;
        self.waiting_user = false;
        self.stopped = None;
        self.reserved = None;
        Ok(())
    }

    /// 查询仅暴露是否许可，不允许序列化 runtime 后在恢复时自动武装。
    pub fn is_armed(&self) -> bool {
        self.armed
    }

    /// 宿主显示等待原因，不能把等待误报为 Goal 完成。
    pub fn is_waiting_user(&self) -> bool {
        self.waiting_user
    }

    /// 保留停止原因供 UI 与日志安全展示，不携带认证或存储原始错误。
    pub fn stop_reason(&self) -> Option<StopReason> {
        self.stopped
    }

    /// 进入批准或提问等待立即撤销预约；子通知不能解除等待。
    pub fn wait_for_user(&mut self) {
        self.waiting_user = true;
        self.armed = false;
        self.reserved = None;
    }

    /// 先停止准入再由宿主取消活动树；即使保存失败也不允许自动恢复。
    pub fn stop(&mut self, reason: StopReason) {
        self.armed = false;
        self.stopped = Some(reason);
        self.reserved = None;
        self.running = None;
    }

    /// 成功保存更新后同步生命周期；active 更新不自行授予人类权限。
    pub fn observe_goal(&mut self, goal: &Goal) {
        if self.goal_id.as_deref() != Some(goal.id.as_str()) || goal.phase != GoalPhase::Active {
            self.armed = false;
            self.reserved = None;
        }
    }

    /// 预约阶段只登记意图；重复调度会被 Busy 拒绝，不增加 roundsStarted。
    pub fn reserve(
        &mut self,
        goal: &Goal,
        admission: &Admission,
    ) -> Result<Reservation, DomainError> {
        self.check_admission(goal, admission)?;
        if self.reserved.is_some() {
            return Err(DomainError::Busy);
        }
        let reservation = Reservation {
            goal_id: goal.id.clone(),
            goal_revision: goal.revision,
            execution_id: self.execution_id.clone(),
            sequence: next_revision(self.sequence)?,
        };
        self.sequence = reservation.sequence;
        self.reserved = Some(reservation.clone());
        Ok(reservation)
    }

    /// 复验后返回已计数候选 Goal；宿主必须与轮次历史一起保存，失败丢弃 runtime 候选。
    pub fn admit(
        &mut self,
        goal: &Goal,
        reservation: &Reservation,
        execution_id: &str,
        admission: &Admission,
    ) -> Result<Goal, DomainError> {
        if self.reserved.as_ref() != Some(reservation) || execution_id != self.execution_id {
            return Err(DomainError::StaleReservation);
        }
        check_cas(goal, &reservation.goal_id, reservation.goal_revision)?;
        self.check_admission(goal, admission)?;
        let mut next = goal.clone();
        next.revision = next_revision(goal.revision)?;
        // 计数仅供展示与阻塞证明使用；饱和而非回绕或将整数容量变成续轮上限。
        next.rounds_started = goal.rounds_started.saturating_add(1);
        self.reserved = None;
        self.running = Some(reservation.clone());
        Ok(next)
    }

    /// 失效预约可撤销且不增加轮数，不允许旧回调撤销新预约。
    pub fn cancel_reservation(&mut self, reservation: &Reservation) -> Result<(), DomainError> {
        if self.reserved.as_ref() != Some(reservation) {
            return Err(DomainError::StaleReservation);
        }
        self.reserved = None;
        Ok(())
    }

    /// 完成当前实际轮次才释放槽位；异常结束永久撤销本次自动许可，不做自动重试。
    pub fn finish_turn(
        &mut self,
        reservation: &Reservation,
        normal: bool,
    ) -> Result<(), DomainError> {
        if self.running.as_ref() != Some(reservation) {
            return Err(DomainError::StaleReservation);
        }
        self.running = None;
        if !normal {
            self.stop(StopReason::AbnormalTurn);
        }
        Ok(())
    }

    /// 两阶段共用相同闸门，用户消息、等待和落盘任何一项变化都阻止准入。
    fn check_admission(&self, goal: &Goal, admission: &Admission) -> Result<(), DomainError> {
        validate_goal(goal)?;
        require_active(goal)?;
        if self.stopped.is_some() {
            return Err(DomainError::Stopped);
        }
        if self.waiting_user {
            return Err(DomainError::WaitingUser);
        }
        if !self.armed || self.goal_id.as_deref() != Some(goal.id.as_str()) {
            return Err(DomainError::Disarmed);
        }
        if admission.human_pending {
            return Err(DomainError::HumanPending);
        }
        if admission.execution_busy || self.running.is_some() {
            return Err(DomainError::Busy);
        }
        if admission.waiting_children {
            return Err(DomainError::WaitingChildren);
        }
        if !admission.persisted {
            return Err(DomainError::NotPersisted);
        }
        if !admission.normal_turn_end {
            return Err(DomainError::AbnormalTurn);
        }
        Ok(())
    }
}
