//! 派生工具只转交宿主提供的当前身份，模型不能指定 caller。
use super::*;

/// 子 Agent 也获得相同工具，递归由管理器的深度与总量约束。
///
/// writeScope 是父级对子代理的落盘授权：缺省或空数组 = 只读；
/// 元素以 `/` 结尾表示目录前缀（可在其中新建笔记），否则是精确笔记文件（只允许修改）。
pub(super) fn registry() -> Vec<RuntimeTool> {
    let properties = json!({"prompt":{"type":"string"},"description":{"type":"string"},"paths":{"type":"array","items":{"type":"string"}},"writeScope":{"type":"array","items":{"type":"string"}}});
    vec![
        tool("subagent", "后台派生独立子 Agent；提供完整任务、背景与验收要求，子 Agent 可以继续派生；默认继承允许范围；writeScope 把落盘路径授权给子代理，让它自己写笔记而不是把正文回传", properties.clone(), &["prompt","description"], spawn),
        tool("subagent_fork", "后台分叉已完成轮次的上下文；不包含当前轮次，当前要求必须写入 prompt；不能缩窄继承资料的权限；writeScope 同样不得超过父级授权", properties, &["prompt","description"], fork),
        tool("send_message", "给直接子或直接父追加消息，只返回接收确认；完成结果随后通知", json!({"agentId":{"type":"string"},"message":{"type":"string"}}), &["agentId","message"], send),
        tool("list_agents", "列出可续聊的直接子 Agent，descendants 为 true 列出后代树；不要反复轮询", json!({"descendants":{"type":"boolean"}}), &[], list),
        tool("interrupt_agent", "中断一个后代当前轮次，保留会话及其后代；不会取消整棵树", json!({"agentId":{"type":"string"}}), &["agentId"], interrupt),
    ]
}

/// 独立任务从新上下文开始，派生不复制根 Goal 执行权。
fn spawn<'a, 'b>(
    turn: &'a mut AgentTurn<'b>,
    args: &'a Value,
) -> AgentFuture<'a, tools::ToolOutcome> {
    Box::pin(async move { start(turn, args, false).await })
}
/// Fork 仅复制完整轮次，资料范围缩窄时拒绝继承旧广范围内容。
fn fork<'a, 'b>(
    turn: &'a mut AgentTurn<'b>,
    args: &'a Value,
) -> AgentFuture<'a, tools::ToolOutcome> {
    Box::pin(async move { start(turn, args, true).await })
}
/// 管理器是唯一派生入口，任务 id 由后端生成。
async fn start(
    turn: &mut AgentTurn<'_>,
    args: &Value,
    fork: bool,
) -> Result<tools::ToolOutcome, CommandError> {
    let tree = turn
        .tree
        .as_ref()
        .ok_or_else(|| CommandError::validation("当前未启用子 Agent"))?;
    let paths: Vec<String> = if args["paths"].is_null() {
        vec![]
    } else {
        serde_json::from_value(args["paths"].clone())
            .map_err(|_| CommandError::validation("资料范围无效"))?
    };
    if fork && !paths.is_empty() && paths != turn.session.selected_paths {
        return Err(CommandError::validation(
            "缩窄范围不能继承父历史，请使用 subagent",
        ));
    }
    // 缺省或空数组 = 只读，与从未授权的子代理行为完全一致。
    let requested: Vec<String> = if args["writeScope"].is_null() {
        Vec::new()
    } else {
        serde_json::from_value(args["writeScope"].clone())
            .map_err(|_| CommandError::validation("写入授权范围无效，只接受路径数组"))?
    };
    // 根授予的路径必须经 vaultfs 净化且真实存在；子代理的再授权由管理器的子集校验负责。
    let write_scope = if turn.session.parent_session_id.is_none() {
        turn.ports.repository.validate_write_scope(&requested)?
    } else {
        requested
    };
    let granted = write_scope.clone();
    let id = tree
        .spawn_granted(
            &turn.session.id,
            turn.session,
            args["prompt"].as_str().unwrap_or(""),
            args["description"].as_str().unwrap_or(""),
            fork,
            paths,
            write_scope,
        )
        .await?;
    Ok(value(
        json!({"agentId":id,"state":"running","writeScope":granted,"hint":"可继续独立工作；回复结束时宿主会等待相关子任务并投递结果"}),
    ))
}
/// 发送只沿父子边传递，不接受伪造父身份或越级消息。
fn send<'a, 'b>(
    turn: &'a mut AgentTurn<'b>,
    args: &'a Value,
) -> AgentFuture<'a, tools::ToolOutcome> {
    Box::pin(async move {
        let tree = turn
            .tree
            .as_ref()
            .ok_or_else(|| CommandError::validation("当前未启用子 Agent"))?;
        let id = tree
            .send(
                &turn.session.id,
                args["agentId"].as_str().unwrap_or(""),
                args["message"].as_str().unwrap_or(""),
            )
            .await?;
        Ok(value(json!({"messageId":id})))
    })
}
/// 列表是快照，不意味着目标当前一定能接收消息。
fn list<'a, 'b>(
    turn: &'a mut AgentTurn<'b>,
    args: &'a Value,
) -> AgentFuture<'a, tools::ToolOutcome> {
    Box::pin(async move {
        let tree = turn
            .tree
            .as_ref()
            .ok_or_else(|| CommandError::validation("当前未启用子 Agent"))?;
        Ok(value(
            json!({"agents":tree.list(&turn.session.id, args["descendants"].as_bool().unwrap_or(false))?}),
        ))
    })
}
/// 中断接受不表示目标已经收尾，后续状态与通知才是结果。
fn interrupt<'a, 'b>(
    turn: &'a mut AgentTurn<'b>,
    args: &'a Value,
) -> AgentFuture<'a, tools::ToolOutcome> {
    Box::pin(async move {
        let tree = turn
            .tree
            .as_ref()
            .ok_or_else(|| CommandError::validation("当前未启用子 Agent"))?;
        tree.interrupt(&turn.session.id, args["agentId"].as_str().unwrap_or(""))?;
        Ok(value(json!({"accepted":true})))
    })
}
