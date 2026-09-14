//! 纯领域候选转换：宿主必须在同一会话串行边界内比较、保存，再发布结果。
mod completion;
mod plan;
mod runtime;
#[cfg(test)]
mod runtime_tests;
#[cfg(test)]
mod tests;
#[cfg(test)]
mod unlimited_tests;

use crate::agent_autonomy_models::{Goal, GoalBlocker, GoalEvidence, GoalPhase};
pub use completion::{check_completion, CompletionContext, CompletionIssue};
pub use plan::{replace_plan, MAX_PLAN_STEPS};
pub use runtime::{Admission, GoalRuntime, Reservation, StopReason};

// 仅限制单次阻塞证明的载荷体积，不限制目标自动续轮。
const MAX_BLOCKER_ATTEMPTS: usize = 256;
pub const MIN_BLOCKER_ROUNDS: u32 = 3;

/// 来源必须由宿主按真实轮次建立，不能从模型参数或子消息反序列化。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Authority {
    RootUser,
    RootAutomatic,
    Child,
    #[cfg(test)]
    External,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DomainError {
    Unauthorized,
    Conflict,
    InvalidInput,
    InvalidPhase,
    ExistingGoal,
    Incomplete,
    Completion(CompletionIssue),
    BlockerTooEarly { rounds_started: u32 },
    RevisionExhausted,
    Disarmed,
    Stopped,
    WaitingUser,
    WaitingChildren,
    HumanPending,
    Busy,
    NotPersisted,
    AbnormalTurn,
    StaleReservation,
}

#[derive(Clone, Debug)]
pub struct GoalSpec {
    pub objective: String,
    pub acceptance_criteria: Vec<String>,
    /// 废弃惰性字段：兼容旧调用，不参与创建、编辑或续轮准入。
    pub max_goal_rounds: u32,
}

#[derive(Clone, Debug)]
pub enum GoalUpdate {
    Edit(GoalSpec),
    Pause,
    Resume,
    Complete(Vec<GoalEvidence>),
    Block(GoalBlocker),
}

/// 只允许直接人类请求创建；已存在的 Goal（含完成态）须由宿主显式归档后再建。
pub fn create_goal(
    current: Option<&Goal>,
    authority: Authority,
    id: &str,
    spec: GoalSpec,
) -> Result<Goal, DomainError> {
    require_user(authority)?;
    if current.is_some() {
        return Err(DomainError::ExistingGoal);
    }
    text(id, 128)?;
    validate_spec(&spec)?;
    Ok(Goal {
        id: id.into(),
        revision: 1,
        objective: spec.objective,
        acceptance_criteria: spec.acceptance_criteria,
        max_goal_rounds: spec.max_goal_rounds,
        phase: GoalPhase::Active,
        ..Goal::default()
    })
}

/// 所有更新比较准确 id/revision；完成检查显式注入宿主事实与收据验证，不调用评估模型。
pub fn update_goal(
    current: &Goal,
    expected_id: &str,
    expected_revision: u64,
    authority: Authority,
    update: GoalUpdate,
    completion: Option<&CompletionContext<'_>>,
    verify_receipt: impl Fn(&GoalEvidence) -> bool,
) -> Result<Goal, DomainError> {
    check_cas(current, expected_id, expected_revision)?;
    validate_goal(current)?;
    if !matches!(authority, Authority::RootUser | Authority::RootAutomatic) {
        return Err(DomainError::Unauthorized);
    }
    if current.phase == GoalPhase::Complete {
        return Err(DomainError::InvalidPhase);
    }
    let mut next = current.clone();
    match update {
        GoalUpdate::Edit(spec) => {
            require_user(authority)?;
            validate_spec(&spec)?;
            next.objective = spec.objective;
            next.acceptance_criteria = spec.acceptance_criteria;
            next.max_goal_rounds = spec.max_goal_rounds;
            next.evidence.clear();
            next.blocker = None;
        }
        GoalUpdate::Pause => {
            require_user(authority)?;
            next.phase = GoalPhase::Paused;
        }
        GoalUpdate::Resume => {
            require_user(authority)?;
            next.phase = GoalPhase::Active;
            next.blocker = None;
        }
        GoalUpdate::Complete(evidence) => {
            require_active(current)?;
            check_completion(
                current,
                &evidence,
                completion.ok_or(DomainError::Incomplete)?,
                verify_receipt,
            )?;
            next.phase = GoalPhase::Complete;
            next.evidence = evidence;
            next.blocker = None;
        }
        GoalUpdate::Block(blocker) => {
            require_active(current)?;
            validate_blocker(current, &blocker)?;
            next.phase = GoalPhase::Blocked;
            next.blocker = Some(blocker);
        }
    }
    next.revision = next_revision(current.revision)?;
    Ok(next)
}

/// 模型阻塞必须提供最近连续至少三轮尝试；同障碍语义不由字符串比较假装证明。
fn validate_blocker(goal: &Goal, blocker: &GoalBlocker) -> Result<(), DomainError> {
    text(&blocker.reason, 4000)?;
    let count = blocker.attempts.len();
    if goal.rounds_started < MIN_BLOCKER_ROUNDS || count < MIN_BLOCKER_ROUNDS as usize {
        return Err(DomainError::BlockerTooEarly {
            rounds_started: goal.rounds_started,
        });
    }
    if count > goal.rounds_started as usize || count > MAX_BLOCKER_ATTEMPTS {
        return Err(DomainError::InvalidInput);
    }
    let first = goal.rounds_started - count as u32 + 1;
    for (index, attempt) in blocker.attempts.iter().enumerate() {
        if attempt.round != first + index as u32
            || attempt.result_refs.is_empty()
            || attempt.result_refs.len() > 64
        {
            return Err(DomainError::InvalidInput);
        }
        for reference in &attempt.result_refs {
            text(reference, 512)?;
        }
    }
    Ok(())
}

/// 执行前要求有效身份与规格；旧轮数上限不影响合法目标恢复。
pub(super) fn validate_goal(goal: &Goal) -> Result<(), DomainError> {
    text(&goal.id, 128)?;
    if goal.revision == 0 {
        return Err(DomainError::InvalidInput);
    }
    validate_spec(&GoalSpec {
        objective: goal.objective.clone(),
        acceptance_criteria: goal.acceptance_criteria.clone(),
        max_goal_rounds: goal.max_goal_rounds,
    })
}

/// 仅限制目标文本体积；旧 max_goal_rounds 的任何值都不构成续轮额度。
fn validate_spec(spec: &GoalSpec) -> Result<(), DomainError> {
    text(&spec.objective, 8000)?;
    if spec.acceptance_criteria.is_empty() || spec.acceptance_criteria.len() > 64 {
        return Err(DomainError::InvalidInput);
    }
    for criterion in &spec.acceptance_criteria {
        text(criterion, 2000)?;
    }
    Ok(())
}

/// CAS 失败不得修改输入或消耗修订。
pub(super) fn check_cas(goal: &Goal, id: &str, revision: u64) -> Result<(), DomainError> {
    if goal.id != id || goal.revision != revision {
        return Err(DomainError::Conflict);
    }
    Ok(())
}

/// 自动消息和子消息不能扩大人类授权。
pub(super) fn require_user(authority: Authority) -> Result<(), DomainError> {
    if authority != Authority::RootUser {
        return Err(DomainError::Unauthorized);
    }
    Ok(())
}

/// 非活动目标不能通过完成或阻塞动作偷偷恢复执行。
pub(super) fn require_active(goal: &Goal) -> Result<(), DomainError> {
    if goal.phase != GoalPhase::Active {
        return Err(DomainError::InvalidPhase);
    }
    Ok(())
}

/// 修订溢出必须显式失败，不允许回绕后接受旧写入。
pub(super) fn next_revision(revision: u64) -> Result<u64, DomainError> {
    revision
        .checked_add(1)
        .ok_or(DomainError::RevisionExhausted)
}

/// 字符界限适用于中文，空白内容不能作为目标或凭据引用。
pub(super) fn text(value: &str, max: usize) -> Result<(), DomainError> {
    if value.trim().is_empty() || value.chars().count() > max {
        return Err(DomainError::InvalidInput);
    }
    Ok(())
}
