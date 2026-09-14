//! 执行计划由会话独占，整表替换同时保留旧界面使用的 text 字段。
use super::*;
use crate::agent_autonomy_models::{PlanStatus, PlanStep};
use crate::services::agent_goal::replace_plan;

/// 计划读取与更新使用同一个修订来源，允许真实并行执行。
pub(super) fn registry() -> Vec<RuntimeTool> {
    vec![
        tool(
            "get_plan",
            "读取当前执行计划与 planRevision，更新前先读取",
            json!({}),
            &[],
            get,
        ),
        tool(
            "update_plan",
            "整表更新执行计划；先完成实际核验并更新步骤，再提交 Goal。不要把提交 Goal 本身设为必需步骤。resultRefs 仅记录产物来源，不作为验收证据；required 默认 true，仅真正可选步骤设 false，不能用它绕过验收条件",
            json!({
                "planRevision":{"type":"integer","minimum":0},
                "steps":{"type":"array","minItems":1,"maxItems":crate::services::agent_goal::MAX_PLAN_STEPS,"items":{"type":"object","properties":{
                    "id":{"type":"string"},"text":{"type":"string"},
                    "required":{"type":"boolean","description":"默认 true；仅不影响目标验收的可选工作可设 false"},
                    "status":{"type":"string","enum":["pending","in_progress","completed","blocked","cancelled"]},
                    "childAgentId":{"type":"string"},"dependencies":{"type":"array","items":{"type":"string"}},
                    "resultRefs":{"type":"array","description":"产物追踪引用（如 task:/change:/subagent:），不是 Goal evidence，也无需全部提交验收","items":{"type":"string"}}
                },"required":["id","text","status"],"additionalProperties":false}}
            }),
            &["planRevision", "steps"],
            update,
        ),
    ]
}

/// 读取不会授权修改其他会话，也不会把全勾选变成 Goal 完成。
fn get<'a, 'b>(turn: &'a mut AgentTurn<'b>, _: &'a Value) -> AgentFuture<'a, tools::ToolOutcome> {
    Box::pin(async move {
        Ok(value(
            json!({"planRevision":turn.session.plan.revision,"plan":turn.session.plan}),
        ))
    })
}

/// 先验证候选并保存，再发布内存状态；历史快照只追加，避免破坏 Fork 边界。
fn update<'a, 'b>(
    turn: &'a mut AgentTurn<'b>,
    args: &'a Value,
) -> AgentFuture<'a, tools::ToolOutcome> {
    Box::pin(async move {
        let revision = args["planRevision"]
            .as_u64()
            .ok_or_else(|| CommandError::validation("请提交 planRevision"))?;
        let rows = args["steps"]
            .as_array()
            .ok_or_else(|| CommandError::validation("请提交 steps"))?;
        let mut steps = Vec::new();
        for row in rows {
            let status: PlanStatus = serde_json::from_value(row["status"].clone())
                .map_err(|_| CommandError::validation("计划状态无效"))?;
            let strings = |key: &str| -> Result<Vec<String>, CommandError> {
                if row[key].is_null() {
                    return Ok(Vec::new());
                }
                serde_json::from_value(row[key].clone())
                    .map_err(|_| CommandError::validation("计划引用无效"))
            };
            steps.push(PlanStep {
                id: row["id"].as_str().unwrap_or("").into(),
                content: row["text"].as_str().unwrap_or("").into(),
                status,
                required: row["required"].as_bool().unwrap_or(true),
                dependencies: strings("dependencies")?,
                result_refs: strings("resultRefs")?,
                child_agent_id: row["childAgentId"]
                    .as_str()
                    .filter(|id| !id.is_empty())
                    .map(String::from),
            });
        }
        let plan = replace_plan(
            &turn.session.plan,
            &turn.session.id,
            &turn.session.id,
            revision,
            steps,
        )
        .map_err(domain_error)?;
        if let Some(tree) = &turn.tree {
            let children = tree.list(&turn.session.id, false)?;
            if plan
                .steps
                .iter()
                .filter_map(|s| s.child_agent_id.as_ref())
                .any(|id| !children.iter().any(|c| &c.agent_id == id))
            {
                return Err(CommandError::validation("计划引用的子 Agent 不属于本会话"));
            }
        } else if plan.steps.iter().any(|s| s.child_agent_id.is_some()) {
            return Err(CommandError::validation("当前没有可引用的子 Agent"));
        }
        let mut candidate = turn.session.clone();
        candidate.plan = plan;
        let display = candidate
            .plan
            .steps
            .iter()
            .map(|s| json!({"id":s.id,"text":s.content,"status":s.status}))
            .collect::<Vec<_>>();
        candidate.messages.push(tools::block(
            "plan",
            "当前计划",
            json!({"steps":display,"planRevision":candidate.plan.revision}),
        ));
        turn.ports.repository.save_session(&candidate)?;
        *turn.session = candidate;
        Ok(value(
            json!({"planRevision":turn.session.plan.revision,"plan":turn.session.plan}),
        ))
    })
}
