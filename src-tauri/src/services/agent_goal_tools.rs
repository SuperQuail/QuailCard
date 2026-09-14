//! Goal 工具只操作当前根会话，执行来源由宿主提供。
#[cfg(test)]
#[path = "agent_goal/tool_tests.rs"]
mod tests;
use super::*;
use crate::agent_autonomy_models::{GoalBlocker, GoalEvidence, GoalPhase};
use crate::services::agent_goal::{self, Authority, CompletionContext, GoalSpec, GoalUpdate};

/// 根目标工具统一注册，子调用仍在领域层拒绝，不能只依赖工具隐藏。
pub(super) fn registry() -> Vec<RuntimeTool> {
    vec![
        tool(
            "get_goal",
            "读取当前 Goal、准确修订、sourceVersion 和可用收据；更新前必须读取",
            json!({}),
            &[],
            get,
        ),
        tool(
            "create_goal",
            "从直接用户的多步请求建立完成目标；普通问答不创建，子 Agent 不可创建",
            json!({
                "objective":{"type":"string"},"acceptanceCriteria":{"type":"array","items":{"type":"string"}}
            }),
            &["objective", "acceptanceCriteria"],
            create,
        ),
        tool(
            "update_goal",
            "按准确 goalId/revision 更新目标；先完成必需计划步骤并处理子任务，再按每条验收条件提交实际 tool:/message: 收据。resultRefs 的产物标识无需全部提交，且不能充当 evidence；同一有效收据可支持多条条件。blocked 按真实目标续轮数计数，不按工具重试次数",
            json!({
                "goalId":{"type":"string"},"revision":{"type":"integer","minimum":1},
                "action":{"type":"string","enum":["edit","pause","resume","complete","blocked"]},
                "objective":{"type":"string"},"acceptanceCriteria":{"type":"array","items":{"type":"string"}},
                "evidence":{"type":"array","items":{"type":"object","properties":{
                    "criterionIndex":{"type":"integer","minimum":0},"goalRevision":{"type":"integer","minimum":1},
                    "sourceVersion":{"type":"string"},"receiptRef":{"type":"string"}
                },"required":["criterionIndex","goalRevision","sourceVersion","receiptRef"],"additionalProperties":false}},
                "blocker":{"type":"object","properties":{"reason":{"type":"string"},"attempts":{"type":"array","items":{"type":"object","properties":{
                    "round":{"type":"integer","minimum":1},"resultRefs":{"type":"array","items":{"type":"string"}}
                },"required":["round","resultRefs"],"additionalProperties":false}}},"required":["reason","attempts"],"additionalProperties":false}
            }),
            &["goalId", "revision", "action"],
            update,
        ),
        tool(
            "wait_for_user",
            "只有确需用户批准、采纳或回答时挂起；尚有可自主完成工作不能用它早停",
            json!({"question":{"type":"string"}}),
            &["question"],
            wait_user,
        ),
    ]
}

/// 当前授权版本绑定本次用户请求，不接受子消息冒充真人。
fn get<'a, 'b>(turn: &'a mut AgentTurn<'b>, _: &'a Value) -> AgentFuture<'a, tools::ToolOutcome> {
    Box::pin(async move { Ok(value(snapshot(turn))) })
}

/// 返回真实收据引用而不是要求模型猜测消息身份。
fn snapshot(turn: &AgentTurn<'_>) -> Value {
    json!({"goal":turn.session.goal,"writeScope":turn.session.write_scope,"armed":turn.goal_runtime.is_armed(),
        "sourceVersion":turn.input.request_id,"receipts":receipts::available(turn),"planRevision":turn.session.plan.revision,
        "pendingDrafts":receipts::pending_drafts(turn),"waitingUser":turn.goal_runtime.is_waiting_user(),"stopReason":turn.goal_runtime.stop_reason().map(|reason|format!("{reason:?}"))})
}

/// 创建时把验收条件和初始空计划归属一起保存，完成旧目标仅作为历史保留。
fn create<'a, 'b>(
    turn: &'a mut AgentTurn<'b>,
    args: &'a Value,
) -> AgentFuture<'a, tools::ToolOutcome> {
    Box::pin(async move {
        let current = turn
            .session
            .goal
            .as_ref()
            .filter(|g| g.phase != GoalPhase::Complete);
        let next = agent_goal::create_goal(
            current,
            turn.authority,
            &uuid::Uuid::now_v7().to_string(),
            spec(args, 0)?,
        )
        .map_err(domain_error)?;
        let mut runtime = turn.goal_runtime.clone();
        runtime.arm(&next, turn.authority).map_err(domain_error)?;
        let mut candidate = turn.session.clone();
        candidate.goal = Some(next);
        candidate.plan = crate::agent_autonomy_models::Plan {
            owner_session_id: candidate.id.clone(),
            ..Default::default()
        };
        candidate.messages.push(tools::block(
            "status",
            "已建立目标，将在未完成时自动继续",
            json!({"goal":candidate.goal,"goalReset":true}),
        ));
        turn.ports.repository.save_session(&candidate)?;
        *turn.session = candidate;
        turn.goal_runtime = runtime;
        Ok(value(snapshot(turn)))
    })
}

/// 新目标使用零惰性值；编辑保留旧字段，模型不能设置续轮额度。
fn spec(args: &Value, legacy_max_goal_rounds: u32) -> Result<GoalSpec, CommandError> {
    Ok(GoalSpec {
        objective: args["objective"].as_str().unwrap_or("").into(),
        acceptance_criteria: serde_json::from_value(args["acceptanceCriteria"].clone())
            .map_err(|_| CommandError::validation("请提供验收条件"))?,
        max_goal_rounds: legacy_max_goal_rounds,
    })
}

type UpdateParser = fn(&AgentTurn<'_>, &Value) -> Result<GoalUpdate, CommandError>;
/// 动作通过注册表扩展，避免工具主循环堆叠命令分支。
fn parse_update(turn: &AgentTurn<'_>, args: &Value) -> Result<GoalUpdate, CommandError> {
    let actions: &[(&str, UpdateParser)] = &[
        ("edit", edit_action),
        ("pause", pause_action),
        ("resume", resume_action),
        ("complete", complete_action),
        ("blocked", block_action),
    ];
    let action = actions
        .iter()
        .find(|(name, _)| Some(*name) == args["action"].as_str())
        .ok_or_else(|| CommandError::validation("目标动作无效"))?
        .1;
    action(turn, args)
}
/// 编辑只接受明确的新规格，由领域层验证人类来源。
fn edit_action(turn: &AgentTurn<'_>, args: &Value) -> Result<GoalUpdate, CommandError> {
    let legacy_max = turn
        .session
        .goal
        .as_ref()
        .map_or(0, |goal| goal.max_goal_rounds);
    Ok(GoalUpdate::Edit(spec(args, legacy_max)?))
}
/// 暂停保留持久化目标，不能销毁用户工作记录。
fn pause_action(_: &AgentTurn<'_>, _: &Value) -> Result<GoalUpdate, CommandError> {
    Ok(GoalUpdate::Pause)
}
/// 恢复只重新武装当前用户已授权的目标。
fn resume_action(_: &AgentTurn<'_>, _: &Value) -> Result<GoalUpdate, CommandError> {
    Ok(GoalUpdate::Resume)
}
/// 结构化证据必须能由当前会话事实验证。
fn complete_action(_: &AgentTurn<'_>, args: &Value) -> Result<GoalUpdate, CommandError> {
    let evidence: Vec<GoalEvidence> = serde_json::from_value(args["evidence"].clone())
        .map_err(|_| CommandError::validation("完成需要 evidence"))?;
    Ok(GoalUpdate::Complete(evidence))
}
/// 阻塞尝试引用必须来自真实会话结果，不能伪造不存在的工具收据。
fn block_action(turn: &AgentTurn<'_>, args: &Value) -> Result<GoalUpdate, CommandError> {
    let blocker: GoalBlocker = serde_json::from_value(args["blocker"].clone())
        .map_err(|_| CommandError::validation("阻塞需要 reason 和 attempts"))?;
    if blocker
        .attempts
        .iter()
        .flat_map(|a| &a.result_refs)
        .any(|id| !receipts::exists(turn, id, false))
    {
        return Err(CommandError::validation("阻塞尝试引用不存在"));
    }
    Ok(GoalUpdate::Block(blocker))
}

/// 候选状态只有持久化成功才发布；完成仍受活动子任务、消息和必要步骤约束。
fn update<'a, 'b>(
    turn: &'a mut AgentTurn<'b>,
    args: &'a Value,
) -> AgentFuture<'a, tools::ToolOutcome> {
    Box::pin(async move {
        let goal = turn
            .session
            .goal
            .as_ref()
            .ok_or_else(|| CommandError::validation("当前没有目标"))?;
        let action = parse_update(turn, args)?;
        if matches!(action, GoalUpdate::Complete(_))
            && (turn.waiting_user || turn.generation.is_some() || receipts::pending_drafts(turn))
        {
            return Err(CommandError::new(
                "GOAL_INCOMPLETE",
                "仍需用户采纳或批准，不能将草稿当作完成",
            ));
        }
        let resume = matches!(action, GoalUpdate::Resume);
        let redefine = matches!(action, GoalUpdate::Edit(_));
        let pending = turn
            .tree
            .as_ref()
            .is_some_and(|tree| tree.has_pending(&turn.session.id));
        let context = CompletionContext {
            plan: &turn.session.plan,
            owner_session_id: &turn.session.id,
            pending_children: usize::from(pending),
            pending_messages: 0,
            descendants_settled: !pending,
            source_version: &turn.input.request_id,
        };
        let next = agent_goal::update_goal(
            goal,
            args["goalId"].as_str().unwrap_or(""),
            args["revision"].as_u64().unwrap_or(0),
            turn.authority,
            action,
            Some(&context),
            |e| receipts::exists(turn, &e.receipt_ref, true),
        )
        .map_err(domain_error)?;
        let mut runtime = turn.goal_runtime.clone();
        runtime.observe_goal(&next);
        if resume {
            runtime.arm(&next, turn.authority).map_err(domain_error)?;
        }
        let terminal = next.phase != GoalPhase::Active;
        // 冻结和 pending 核验同锁执行；保存失败时 RAII 撤销本次冻结。
        let lease: Option<crate::services::subagents::TerminalLease> = if terminal
            && !turn.tree_sealed
        {
            turn.tree
                .as_ref()
                .map(|tree| tree.seal_terminal(&turn.session.id, next.phase == GoalPhase::Complete))
                .transpose()?
        } else {
            None
        };
        let mut candidate = turn.session.clone();
        candidate.goal = Some(next);
        candidate.messages.push(tools::block(
            "status",
            "目标状态已更新",
            json!({"goal":candidate.goal,"goalReset":redefine}),
        ));
        turn.ports.repository.save_session(&candidate)?;
        *turn.session = candidate;
        turn.goal_runtime = runtime;
        turn.terminal_goal = terminal;
        if let Some(lease) = lease {
            lease.commit();
            turn.tree_sealed = true;
        }
        Ok(value(snapshot(turn)))
    })
}

/// 真正需要人类交互才挂起根执行；子 Agent 应向父级报告而非索要用户授权。
fn wait_user<'a, 'b>(
    turn: &'a mut AgentTurn<'b>,
    args: &'a Value,
) -> AgentFuture<'a, tools::ToolOutcome> {
    Box::pin(async move {
        if turn.authority == Authority::Child {
            return Err(CommandError::validation("子 Agent 请向父级报告问题"));
        }
        let question = args["question"].as_str().unwrap_or("").trim();
        if question.is_empty() || question.chars().count() > 4000 {
            return Err(CommandError::validation("请提供明确的用户问题"));
        }
        turn.seal_tree(false)?;
        turn.goal_runtime.wait_for_user();
        turn.waiting_user = true;
        Ok(tools::ToolOutcome {
            value: json!({"state":"waitingUser"}),
            pause: true,
            message: Some(tools::block("text", question, Value::Null)),
            ..Default::default()
        })
    })
}
