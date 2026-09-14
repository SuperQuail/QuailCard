//! 会话状态工具使用独立注册表；执行前仍从同一声明校验参数。
use super::*;
use crate::ai::tools::{
    spec::{ToolEffect, ToolSpec},
    validate::validate_arguments,
};
use crate::services::agent_ports::AgentFuture;

#[path = "agent_child_tools.rs"]
mod child;
#[path = "agent_goal_tools.rs"]
mod goal;
#[path = "agent_plan_tools.rs"]
mod plan;
#[path = "agent_receipts.rs"]
pub(super) mod receipts;

pub(super) type Handler =
    for<'a, 'b> fn(&'a mut AgentTurn<'b>, &'a Value) -> AgentFuture<'a, tools::ToolOutcome>;
#[derive(Clone)]
pub(super) struct RuntimeTool {
    pub spec: ToolSpec,
    pub handler: Handler,
}

/// 注册行为而非按名称分支，工具声明和执行器始终同源。
pub(super) fn registry(delegation: bool) -> Vec<RuntimeTool> {
    let mut tools = goal::registry();
    tools.extend(plan::registry());
    if delegation {
        tools.extend(child::registry());
    }
    tools
}

/// 运行态工具只改变所属会话，不直接执行笔记写入。
pub(super) fn tool(
    name: &'static str,
    description: &'static str,
    properties: Value,
    required: &[&str],
    handler: Handler,
) -> RuntimeTool {
    RuntimeTool {
        spec: ToolSpec {
            name,
            description,
            schema: json!({"type":"object","properties":properties,"required":required,"additionalProperties":false}),
            effect: ToolEffect::Read,
        },
        handler,
    }
}

/// 参数校验后才调用处理器，避免绕过声明直接构造状态变更。
pub(super) async fn invoke(
    tool: &RuntimeTool,
    turn: &mut AgentTurn<'_>,
    args: &Value,
) -> Result<tools::ToolOutcome, CommandError> {
    validate_arguments(&tool.spec, args)?;
    if args.to_string().len() > 64 * 1024 {
        return Err(CommandError::validation("Agent 工具参数过大"));
    }
    let outcome = (tool.handler)(turn, args).await?;
    autonomy::project(turn.session, turn.control, turn.waiting_user);
    Ok(outcome)
}

/// 通用安全输出，不把执行器内部状态或凭据暴露给模型。
pub(super) fn value(value: Value) -> tools::ToolOutcome {
    tools::ToolOutcome {
        value,
        ..Default::default()
    }
}

/// 域错误仅映射稳定安全代码；详细实现不进入前端错误。
pub(super) fn domain_error(error: crate::services::agent_goal::DomainError) -> CommandError {
    use crate::services::agent_goal::DomainError;
    let (code, message) = match error {
        DomainError::Conflict | DomainError::StaleReservation => (
            "AGENT_REVISION_CONFLICT",
            "状态已变化，请先读取当前 Goal 或计划修订",
        ),
        DomainError::Unauthorized => (
            "AGENT_AUTHORITY_DENIED",
            "此操作需要直接用户授权，子 Agent 或自动消息不能授权",
        ),
        DomainError::Incomplete => (
            "GOAL_INCOMPLETE",
            "缺少宿主完成核验上下文，无法验收；请报告此错误",
        ),
        DomainError::Completion(issue) => return completion_error(issue),
        DomainError::BlockerTooEarly { rounds_started } => return CommandError::new(
            "GOAL_BLOCKER_TOO_EARLY",
            format!("当前 roundsStarted={rounds_started}；同一障碍需要至少 {} 个真实目标续轮及连续尝试记录。工具重试不增加轮数；确需用户决定时使用 wait_for_user。", crate::services::agent_goal::MIN_BLOCKER_ROUNDS),
        ),
        DomainError::ExistingGoal => ("GOAL_EXISTS", "当前已有未完成目标，请先读取并处理它"),
        _ => (
            "AGENT_STATE_INVALID",
            "状态或参数无效，请读取当前状态后修正",
        ),
    };
    CommandError::new(code, message)
}

/// 只报告已校验的分类与零基下标，避免原始错误泄露并让模型定向修正。
fn completion_error(issue: crate::services::agent_goal::CompletionIssue) -> CommandError {
    use crate::services::agent_goal::CompletionIssue;
    let message = match issue {
        CompletionIssue::PlanOwner => "计划归属与当前会话不符，无法验收；请报告用户检查会话状态".into(),
        CompletionIssue::PendingWork => "仍有子任务或消息未收尾，请等待并处理结果后再完成目标".into(),
        CompletionIssue::RequiredStep(index) => format!("计划 steps[{index}] 尚未完成（下标从 0 开始）。请 get_plan，完成实际工作后 update_plan；不要把提交目标本身作为必需的未完成步骤。"),
        CompletionIssue::EvidenceCount => "evidence 必须包含 1–256 条真实收据，并覆盖全部验收条件".into(),
        CompletionIssue::EvidenceRevision(index) => format!("evidence[{index}].goalRevision 已过期，请 get_goal 后使用当前修订"),
        CompletionIssue::EvidenceSource(index) => format!("evidence[{index}].sourceVersion 不属于当前请求，请 get_goal 后使用当前 sourceVersion"),
        CompletionIssue::EvidenceCriterion(index) => format!("evidence[{index}].criterionIndex 超出验收条件范围，下标从 0 开始"),
        CompletionIssue::EvidenceReceipt(index) => format!("evidence[{index}].receiptRef 无效或已过期：仅支持当前目标的有效 tool:/message: 收据；请从 get_goal 的 receipts 选取并按需重新读取材料。计划 resultRefs 的 task:/change:/subagent: 等产物标识不是验收收据，也不必全部提交。"),
        CompletionIssue::MissingCriterion(index) => format!("缺少 acceptanceCriteria[{index}] 的验收证据，请补充 criterionIndex={index} 的有效收据；无须修改验收措辞"),
    };
    CommandError::new("GOAL_INCOMPLETE", message)
}
