use super::{next_revision, text, DomainError};
use crate::agent_autonomy_models::{Plan, PlanStep};
use std::collections::HashSet;

pub const MAX_PLAN_STEPS: usize = 128;

/// 仅计划 owner 可整表替换；比较修订后返回候选值，由宿主锁内持久化再发布。
pub fn replace_plan(
    current: &Plan,
    owner_session_id: &str,
    actor_session_id: &str,
    expected_revision: u64,
    steps: Vec<PlanStep>,
) -> Result<Plan, DomainError> {
    text(owner_session_id, 128)?;
    if actor_session_id != owner_session_id
        || (!current.owner_session_id.is_empty() && current.owner_session_id != owner_session_id)
    {
        return Err(DomainError::Unauthorized);
    }
    if current.revision != expected_revision {
        return Err(DomainError::Conflict);
    }
    validate_steps(&steps)?;
    Ok(Plan {
        owner_session_id: owner_session_id.into(),
        revision: next_revision(current.revision)?,
        steps,
    })
}

/// 验证引用完整和无环；不限制进行中数量，允许真实并行步骤。
pub(super) fn validate_steps(steps: &[PlanStep]) -> Result<(), DomainError> {
    if steps.len() > MAX_PLAN_STEPS {
        return Err(DomainError::InvalidInput);
    }
    let mut ids = HashSet::new();
    for step in steps {
        text(&step.id, 128)?;
        text(&step.content, 4000)?;
        if !ids.insert(step.id.as_str())
            || step.dependencies.len() > MAX_PLAN_STEPS
            || step.result_refs.len() > 64
        {
            return Err(DomainError::InvalidInput);
        }
        if let Some(id) = &step.child_agent_id {
            text(id, 128)?;
        }
        for reference in &step.result_refs {
            text(reference, 512)?;
        }
    }
    for step in steps {
        let mut dependencies = HashSet::new();
        for id in &step.dependencies {
            if id == &step.id || !ids.contains(id.as_str()) || !dependencies.insert(id) {
                return Err(DomainError::InvalidInput);
            }
        }
    }
    let mut resolved = HashSet::new();
    loop {
        let before = resolved.len();
        for step in steps {
            if step
                .dependencies
                .iter()
                .all(|id| resolved.contains(id.as_str()))
            {
                resolved.insert(step.id.as_str());
            }
        }
        if resolved.len() == steps.len() {
            return Ok(());
        }
        if resolved.len() == before {
            return Err(DomainError::InvalidInput);
        }
    }
}
