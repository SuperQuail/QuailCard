use super::{plan::validate_steps, text, validate_goal, DomainError};
use crate::agent_autonomy_models::{Goal, GoalEvidence, Plan, PlanStatus};

/// 必须来自当前执行树与存储的宿主快照，不能直接相信工具入参中的布尔声明。
pub struct CompletionContext<'a> {
    pub plan: &'a Plan,
    pub owner_session_id: &'a str,
    pub pending_children: usize,
    pub pending_messages: usize,
    pub descendants_settled: bool,
    pub source_version: &'a str,
}

/// 只携带安全的数组下标与分类，适配层可诊断失败而不泄露原始存储错误。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CompletionIssue {
    PlanOwner,
    PendingWork,
    RequiredStep(usize),
    EvidenceCount,
    EvidenceRevision(usize),
    EvidenceSource(usize),
    EvidenceCriterion(usize),
    EvidenceReceipt(usize),
    MissingCriterion(usize),
}

/// 产物引用只用于计划追踪；验收独立验证当前证据覆盖，历史引用不升级为硬前置。
/// 仅验证可机械判断的完成条件；内容覆盖度仍由主 Agent 承担，无独立 evaluator。
pub fn check_completion(
    goal: &Goal,
    evidence: &[GoalEvidence],
    context: &CompletionContext<'_>,
    verify_receipt: impl Fn(&GoalEvidence) -> bool,
) -> Result<(), DomainError> {
    validate_goal(goal)?;
    text(context.owner_session_id, 128)?;
    text(context.source_version, 512)?;
    validate_steps(&context.plan.steps)?;
    if context.plan.owner_session_id != context.owner_session_id {
        return Err(DomainError::Completion(CompletionIssue::PlanOwner));
    }
    if context.pending_children != 0
        || context.pending_messages != 0
        || !context.descendants_settled
    {
        return Err(DomainError::Completion(CompletionIssue::PendingWork));
    }
    if let Some(index) = context
        .plan
        .steps
        .iter()
        .position(|step| step.required && step.status != PlanStatus::Completed)
    {
        return Err(DomainError::Completion(CompletionIssue::RequiredStep(
            index,
        )));
    }
    if evidence.is_empty() || evidence.len() > 256 {
        return Err(DomainError::Completion(CompletionIssue::EvidenceCount));
    }
    let mut covered = vec![false; goal.acceptance_criteria.len()];
    for (index, item) in evidence.iter().enumerate() {
        let issue = if item.goal_revision != goal.revision {
            Some(CompletionIssue::EvidenceRevision(index))
        } else if item.source_version != context.source_version {
            Some(CompletionIssue::EvidenceSource(index))
        } else if item.criterion_index as usize >= covered.len() {
            Some(CompletionIssue::EvidenceCriterion(index))
        } else if text(&item.receipt_ref, 512).is_err() || !verify_receipt(item) {
            Some(CompletionIssue::EvidenceReceipt(index))
        } else {
            None
        };
        if let Some(issue) = issue {
            return Err(DomainError::Completion(issue));
        }
        covered[item.criterion_index as usize] = true;
    }
    if let Some(index) = covered.iter().position(|covered| !covered) {
        return Err(DomainError::Completion(CompletionIssue::MissingCriterion(
            index,
        )));
    }
    Ok(())
}
