//! Agent 工具处理器：笔记、学习、词典与记忆。声明与注册表在 agent_tools。

use super::{block, ToolContext, ToolOutcome};
use crate::error::CommandError;
use crate::services::agent_ports::AgentFuture;
use crate::services::generation_dictionary::resolve_lookup_result;
use serde_json::{json, Value};

/// 路径统一正斜线，范围判断不能被大小写或路径别名绕过。
pub(super) fn scoped(scope: &[String], path: &str) -> Result<(), CommandError> {
    if path.contains('\\') || (!scope.is_empty() && !scope.iter().any(|p| p == path)) {
        return Err(CommandError::new(
            "AGENT_SCOPE_DENIED",
            "该笔记不在本轮允许范围内",
        ));
    }
    Ok(())
}

/// 字符串参数已经过 Schema 校验。
pub(super) fn string<'a>(args: &'a Value, key: &str) -> &'a str {
    args[key].as_str().unwrap_or("")
}

/// 搜索由仓库保证不返回所选范围之外的内容。
pub(super) fn search(context: &ToolContext<'_>, args: &Value) -> Result<ToolOutcome, CommandError> {
    Ok(ToolOutcome {
        value: context
            .repository
            .search(string(args, "query"), context.scope)?,
        ..Default::default()
    })
}

/// 单次读取返回的最大行数；超大笔记必须按 nextOffset 分页，避免一次灌满上下文。
pub(super) const READ_LINE_LIMIT: usize = 2_000;

/// 读取前检查范围，路径安全由仓库再次验证；返回带行号的窗口与继续读的 nextOffset。
pub(super) fn read(context: &ToolContext<'_>, args: &Value) -> Result<ToolOutcome, CommandError> {
    let path = string(args, "path");
    scoped(context.scope, path)?;
    let note = context.repository.read(path)?;
    let text = note["content"].as_str().unwrap_or("");
    let offset = (args["offset"].as_u64().unwrap_or(1).max(1) as usize).saturating_sub(1);
    let limit = args["limit"]
        .as_u64()
        .unwrap_or(READ_LINE_LIMIT as u64)
        .clamp(1, READ_LINE_LIMIT as u64) as usize;
    let lines = text.split('\n').collect::<Vec<_>>();
    let total = lines.len();
    let start = offset.min(total);
    let end = (start + limit).min(total);
    let window = lines[start..end]
        .iter()
        .enumerate()
        .map(|(index, line)| json!({"number": start + index + 1, "text": line}))
        .collect::<Vec<_>>();
    Ok(ToolOutcome {
        value: json!({
            "path": path,
            "hash": note["hash"],
            "offset": start + 1,
            "totalLines": total,
            "lines": window,
            "nextOffset": (end < total).then_some(end + 1),
        }),
        message: Some(block("note", path, json!({"path":path}))),
        pause: false,
        generation: None,
        images: Vec::new(),
    })
}

/// 创建允许新文件，但路径必须是普通 Markdown、落在授权目录前缀内且不存在。
pub(super) fn create(context: &ToolContext<'_>, args: &Value) -> Result<ToolOutcome, CommandError> {
    // 执行期二次校验：definitions 只是声明，写盘前必须按本会话 writeScope 复核。
    context.write_scope.check_create(string(args, "path"))?;
    change(context, args, None)
}

/// 编辑必须持有当前范围内已读取内容的版本，且目标在授权写入范围内。
pub(super) fn edit(context: &ToolContext<'_>, args: &Value) -> Result<ToolOutcome, CommandError> {
    scoped(context.scope, string(args, "path"))?;
    context.write_scope.check_edit(string(args, "path"))?;
    change(context, args, Some(string(args, "expectedHash")))
}

/// 返回精简操作凭据，完整差异从独立记录加载。
fn change(
    context: &ToolContext<'_>,
    args: &Value,
    expected: Option<&str>,
) -> Result<ToolOutcome, CommandError> {
    let change = context.repository.change(
        context.operation,
        string(args, "path"),
        string(args, "content"),
        expected,
    )?;
    let data = json!({"changeId":change.id,"path":change.path,"state":change.state,"afterHash":change.after_hash});
    Ok(ToolOutcome {
        value: data.clone(),
        message: Some(block("change", "笔记改动", data)),
        pause: false,
        generation: None,
        images: Vec::new(),
    })
}

/// 正式复习交给用户交互，不向模型暴露写入评分工具。
pub(super) fn review(context: &ToolContext<'_>, args: &Value) -> Result<ToolOutcome, CommandError> {
    let mut paths = args["paths"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_str().unwrap().to_string())
        .collect::<Vec<_>>();
    if paths.is_empty() {
        paths = context.scope.to_vec();
    }
    for path in &paths {
        scoped(context.scope, path)?;
        context.repository.read(path)?;
    }
    if args["mode"] == "notes" && paths.is_empty() {
        return Err(CommandError::validation("指定笔记复习需要选择笔记"));
    }
    let data = json!({"paths":paths,"includeAll":args["mode"] == "notes"});
    Ok(ToolOutcome {
        value: json!({"status":"waitingForUser"}),
        message: Some(block("review", "正式复习", data)),
        pause: true,
        generation: None,
        images: Vec::new(),
    })
}

/// 拆卡准备必须异步执行：要读取真实笔记并复核 Vault 快照，占位实现只声明契约。
pub(super) fn generate_sync(_: &ToolContext<'_>, _: &Value) -> Result<ToolOutcome, CommandError> {
    Err(CommandError::validation("拆卡准备必须异步执行"))
}

/// 进入拆卡模式：只做准备与校验，生成调用由 Agent 循环在同一 turn 内驱动。
pub(super) fn enter_generation<'a, 'b>(
    context: &'a ToolContext<'b>,
    args: &'a Value,
) -> AgentFuture<'a, ToolOutcome> {
    Box::pin(async move {
        // 范围在后端执行阶段复核，模型不能借准备读取范围外的笔记。
        scoped(context.scope, string(args, "path"))?;
        let prepared = context
            .learning
            .prepare(string(args, "path"), string(args, "kind"))
            .await?;
        Ok(ToolOutcome {
            value: json!({
                "ok": true,
                "mode": "cardGeneration",
                "path": prepared.path,
                "kind": prepared.kind,
                "instruction": "已进入拆卡模式：先用 read_generation_material 分页读取固定材料快照，再用 plan_cards 提交考点与 sourceRange 行范围；有效条目保留，错误仅修正对应 itemId。emit_card 按 itemId 落地，不要重复抄写原文，最后 finish_generation 结束。"
            }),
            generation: Some(prepared),
            ..Default::default()
        })
    })
}

/// 词典工具必须走异步端口，同步占位只用于满足注册表结构。
pub(super) fn dictionary_sync(_: &ToolContext<'_>, _: &Value) -> Result<ToolOutcome, CommandError> {
    Err(CommandError::validation("词典查询必须异步执行"))
}

/// 批量查词复用生成流程的严格校验、截断与未命中占位。
pub(super) fn lookup_dictionary<'a, 'b>(
    context: &'a ToolContext<'b>,
    args: &'a Value,
) -> AgentFuture<'a, ToolOutcome> {
    Box::pin(async move {
        let content = resolve_lookup_result(context.dictionary, args)
            .await
            .content;
        let parsed = serde_json::from_str::<Value>(&content).unwrap_or_else(|_| {
            json!({"ok":false,"error":{"code":"DICTIONARY_ERROR","message":"词典结果解析失败"}})
        });
        if parsed["ok"].as_bool() != Some(true) {
            // 错误码只允许生成流程已知的两个静态值，正文不能构造任意码。
            let code = match parsed["error"]["code"].as_str() {
                Some("INVALID_SCHEMA") => "INVALID_SCHEMA",
                _ => "DICTIONARY_ERROR",
            };
            let message = parsed["error"]["message"]
                .as_str()
                .unwrap_or("词典查询失败")
                .to_string();
            return Err(CommandError::new(code, message));
        }
        Ok(ToolOutcome {
            value: parsed,
            ..Default::default()
        })
    })
}

/// 卡片工具必须走异步端口，同步占位只用于满足注册表结构。
pub(super) fn list_cards_sync(_: &ToolContext<'_>, _: &Value) -> Result<ToolOutcome, CommandError> {
    Err(CommandError::validation("卡片列表必须异步执行"))
}

/// 列出笔记卡片；先验证笔记存在且在允许范围内，模型看不到范围外卡片。
pub(super) fn list_cards<'a, 'b>(
    context: &'a ToolContext<'b>,
    args: &'a Value,
) -> AgentFuture<'a, ToolOutcome> {
    Box::pin(async move {
        let path = string(args, "path");
        scoped(context.scope, path)?;
        context.repository.read(path)?;
        let value = context.cards.list(path).await?;
        Ok(ToolOutcome {
            value,
            ..Default::default()
        })
    })
}

/// 删除卡片必须走异步端口，同步占位只用于满足注册表结构。
pub(super) fn delete_card_sync(
    _: &ToolContext<'_>,
    _: &Value,
) -> Result<ToolOutcome, CommandError> {
    Err(CommandError::validation("删除卡片必须异步执行"))
}

/// 删除笔记下的一张卡片；范围与归属都在后端复核，模型不能跨范围删除。
pub(super) fn delete_card<'a, 'b>(
    context: &'a ToolContext<'b>,
    args: &'a Value,
) -> AgentFuture<'a, ToolOutcome> {
    Box::pin(async move {
        let path = string(args, "path");
        scoped(context.scope, path)?;
        context.repository.read(path)?;
        let value = context.cards.delete(path, string(args, "cardId")).await?;
        // 删除不可撤销：会话里留下可见的卡片块，用户能复查删了哪张。
        let data = json!({
            "path": value["path"],
            "cardId": value["id"],
            "front": value["front"],
            "state": "deleted",
        });
        Ok(ToolOutcome {
            value,
            message: Some(block("card", "已删除卡片", data)),
            ..Default::default()
        })
    })
}

/// 模型只能建议记忆，保存必须由用户点击明确入口。
pub(super) fn remember(_: &ToolContext<'_>, args: &Value) -> Result<ToolOutcome, CommandError> {
    if string(args, "content").chars().count() > 8000 {
        return Err(CommandError::validation("记忆过长"));
    }
    Ok(ToolOutcome {
        value: json!({"status":"waitingForUser"}),
        message: Some(block("memory", "记忆建议", args.clone())),
        pause: true,
        generation: None,
        images: Vec::new(),
    })
}
