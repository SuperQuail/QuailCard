use std::{collections::HashSet, future::Future, pin::Pin};

use serde_json::{json, Value};

use super::{
    generation_dictionary::resolve_lookup_result, generation_ports::DictionaryLookup,
    GenerationControl,
};
use crate::{
    ai::tools::spec::ToolSpec,
    ai::{GenerationSession, ToolArguments, ToolCallBatch, ToolDefinition, ToolMessage},
    error::CommandError,
    models::GenerationInput,
};

#[path = "generation_material.rs"]
mod material;

pub(super) struct RoundResult {
    pub history: Vec<ToolMessage>,
    pub progressed: bool,
    pub finish_reason: Option<String>,
}

struct RoundContext<'a> {
    input: &'a GenerationInput,
    session: &'a mut GenerationSession,
    dictionary: &'a dyn DictionaryLookup,
    control: &'a GenerationControl,
    lookups: &'a mut HashSet<String>,
    progressed: bool,
    finish_reason: Option<String>,
    /// 已执行的非结束调用数与失败数；结束调用不能把自身算成待成功工作。
    total_calls: usize,
    failed_calls: usize,
    dictionary_pending: bool,
}

type HandlerFuture<'a> = Pin<Box<dyn Future<Output = String> + Send + 'a>>;
type Handler = for<'a, 'b> fn(&'a mut RoundContext<'b>, &'a Value) -> HandlerFuture<'a>;

/// 处理器表：只声明名字、执行顺序与执行器，模型可见声明由 ToolSpec 提供。
const HANDLERS: &[(&str, u8, Handler)] = &[
    ("read_generation_material", 0, read_material),
    ("plan_cards", 0, plan),
    ("lookup_words", 1, lookup),
    ("emit_card", 2, emit),
    ("finish_generation", 3, finish),
];

/// 生成器工具表：声明与执行器同源，模型看到的工具集不可能与可执行集分叉。
pub(super) struct GenerationTools {
    entries: Vec<BoundTool>,
}

struct BoundTool {
    spec: ToolSpec,
    order: u8,
    handle: Handler,
}

impl GenerationTools {
    /// 按声明绑定处理器；缺少实现即构造失败，不把不可执行工具交给模型。
    pub(super) fn build(specs: Vec<ToolSpec>) -> Result<Self, CommandError> {
        let mut entries = Vec::with_capacity(specs.len());
        for spec in specs {
            let (order, handle) = HANDLERS
                .iter()
                .find(|(name, _, _)| *name == spec.name)
                .map(|(_, order, handle)| (*order, *handle))
                .ok_or_else(|| {
                    CommandError::new("TOOL_NOT_IMPLEMENTED", "工具声明缺少可执行实现")
                })?;
            entries.push(BoundTool {
                spec,
                order,
                handle,
            });
        }
        Ok(Self { entries })
    }

    /// 模型可见的声明只由本表产出，注册顺序稳定。
    pub(super) fn definitions(&self) -> Vec<ToolDefinition> {
        self.entries
            .iter()
            .map(|entry| entry.spec.definition())
            .collect()
    }

    /// 按名字查执行器；未声明即不可执行。
    fn find(&self, name: &str) -> Option<&BoundTool> {
        self.entries.iter().find(|entry| entry.spec.name == name)
    }

    /// 按名字返回展示用描述；未注册返回 None。Agent 拆卡模式据此判断工具归属。
    pub(super) fn description(&self, name: &str) -> Option<&'static str> {
        self.find(name).map(|entry| entry.spec.description)
    }

    /// 最大执行顺序，决定同轮先查词典、再提交卡片、最后结束。
    fn max_order(&self) -> u8 {
        self.entries
            .iter()
            .map(|entry| entry.order)
            .max()
            .unwrap_or(0)
    }
}

/// 注册表独立执行每个调用，失败兄弟调用不能丢弃已通过校验的卡片。
pub(super) async fn process_generation_round(
    dictionary: &dyn DictionaryLookup,
    input: &GenerationInput,
    session: &mut GenerationSession,
    batch: ToolCallBatch,
    tools: &GenerationTools,
    lookups: &mut HashSet<String>,
    control: &GenerationControl,
) -> RoundResult {
    let mut history = assistant_history(&batch);
    let mut results = vec![String::new(); batch.calls.len()];
    let mut context = RoundContext {
        input,
        session,
        dictionary,
        control,
        lookups,
        progressed: false,
        finish_reason: None,
        total_calls: 0,
        failed_calls: 0,
        dictionary_pending: false,
    };
    for order in 0..=tools.max_order() {
        for (index, call) in batch.calls.iter().enumerate() {
            if context.session.fixed_complete() {
                break;
            }
            let registered = tools.find(&call.name);
            if registered.map_or(0, |tool| tool.order) != order {
                continue;
            }
            let content = match (&call.arguments, registered) {
                (_, None) => error_result("UNKNOWN_TOOL", "工具未注册，请改用可用工具"),
                (ToolArguments::Valid(arguments), Some(tool)) => (tool.handle)(&mut context, arguments).await,
                (ToolArguments::Invalid(error), _) => json!({
                    "ok":false,"error":{"code":"INVALID_JSON","message":"工具参数不是有效 JSON","line":error.line,"column":error.column,"category":error.category},
                    "action":"retry","instruction":"仅修正当前失败调用，不要重复已经成功的工具调用"
                }).to_string(),
            };
            let ok = result_ok(&content);
            if order < tools.max_order() {
                context.total_calls += 1;
                if !ok {
                    context.failed_calls += 1;
                }
            }
            results[index] = content;
        }
    }
    for (call, content) in batch.calls.into_iter().zip(results) {
        history.push(ToolMessage::ToolResult {
            id: call.id,
            content,
        });
    }
    RoundResult {
        history,
        progressed: context.progressed,
        finish_reason: context.finish_reason,
    }
}

/// 读取准备阶段的固定快照，而不是再次读取可能已经修改的笔记。
fn read_material<'a, 'b>(
    context: &'a mut RoundContext<'b>,
    arguments: &'a Value,
) -> HandlerFuture<'a> {
    Box::pin(async move {
        match material::read_material(&context.input.source_text, arguments, context.lookups) {
            Ok((value, progressed)) => {
                context.progressed |= progressed;
                value.to_string()
            }
            Err(message) => error_result("INVALID_MATERIAL_WINDOW", message),
        }
    })
}

/// 局部更新考点清单：进度由服务端持有，模型不再靠记忆维护。
fn plan<'a, 'b>(context: &'a mut RoundContext<'b>, arguments: &'a Value) -> HandlerFuture<'a> {
    Box::pin(async move {
        context
            .control
            .progress("planning", context.session.generated());
        match context
            .session
            .submit_plan(context.input, arguments.clone())
        {
            Ok(summary) => {
                context.progressed |= summary.changed;
                let ok = summary.errors.is_empty();
                let mut value = json!(summary);
                value["ok"] = json!(ok);
                if !ok {
                    value["error"] = json!({"code":"PLAN_ITEMS_INVALID","message":format!("{} 个条目需要修正；有效条目已保留，仅修复 errors 中的 itemId", value["errors"].as_array().map_or(0, Vec::len))});
                }
                value["instruction"] = json!(if ok {
                    "按 pendingItems 的 itemId 调用 emit_card，不要复制来源；已有条目已保留，不要重交整表。"
                } else {
                    "有效条目已保留。仅按 errors 中的 itemId 修正失败条目，不要重交整表；待修复或未落卡条目未清空前不能结束。"
                });
                value.to_string()
            }
            Err(error) => error_result(error.code, &error.message),
        }
    })
}

/// 新有效词典结果重置无进展计数，重复查询不构成进展。
fn lookup<'a, 'b>(context: &'a mut RoundContext<'b>, arguments: &'a Value) -> HandlerFuture<'a> {
    Box::pin(async move {
        context
            .control
            .progress("lookup", context.session.generated());
        let result = resolve_lookup_result(context.dictionary, arguments).await;
        context.dictionary_pending |= result_ok(&result.content);
        for word in result.found_words {
            context.progressed |= context.lookups.insert(word);
        }
        result.content
    })
}

/// 每张合法卡片立即更新真实数量，后续取消也能保留该草稿。
fn emit<'a, 'b>(context: &'a mut RoundContext<'b>, arguments: &'a Value) -> HandlerFuture<'a> {
    Box::pin(async move {
        context
            .control
            .progress("validating", context.session.generated());
        if context.dictionary_pending {
            return error_result(
                "LOOKUP_RESULT_PENDING",
                "请读取本轮词典结果，在下一轮依据真实释义和音标重新提交卡片",
            );
        }
        // 落卡必须对应清单里还没完成的一条：跑题与重复在服务端拦住。
        if !context.session.has_plan() {
            return error_result(
                "PLAN_REQUIRED",
                "请先用 plan_cards 提交完整考点清单，再逐条调用 emit_card",
            );
        }
        let (index, canonical) = match context.session.prepare_emit(arguments.clone()) {
            Ok(prepared) => prepared,
            Err(error) => return error_result(error.code, &error.message),
        };
        match context.session.accept(context.input, canonical) {
            Ok(()) => {
                context.session.mark_plan_emitted(index);
                context.progressed = true;
                context
                    .control
                    .progress("validating", context.session.generated());
                json!({"ok":true,"accepted":true,"generated":context.session.generated(),"remaining":context.session.remaining(),"plan_pending":context.session.pending_keywords().len()}).to_string()
            }
            Err(error) => error_result(error.code, &error.message),
        }
    })
}

/// 结束调用最后处理，避免同轮先结束后提交卡片造成草稿遗漏。
fn finish<'a, 'b>(context: &'a mut RoundContext<'b>, arguments: &'a Value) -> HandlerFuture<'a> {
    Box::pin(async move {
        let reason = arguments
            .as_object()
            .filter(|object| object.len() == 1)
            .and_then(|object| object.get("reason"))
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|reason| !reason.is_empty() && reason.chars().count() <= 500);
        // 一次坏调用不该锁死整个回合：只有整轮全部失败才要求先修正。
        if context.total_calls > 0 && context.failed_calls == context.total_calls {
            return error_result("CORRECTIONS_PENDING", "本轮全部调用失败，请先修正后再结束");
        }
        // 清单未落地完就不允许结束：终止条件由清单定义，不靠轮次上限。
        let pending = context.session.pending_keywords();
        if !pending.is_empty() {
            return error_result(
                "PLAN_INCOMPLETE",
                &format!(
                    "清单还有 {} 条未落地：{}；请继续落地或先更新清单",
                    pending.len(),
                    pending.join("、")
                ),
            );
        }
        let Some(reason) = reason else {
            return error_result(
                "INVALID_SCHEMA",
                "finish_generation 必须提供 1-500 字符的 reason",
            );
        };
        context.finish_reason = Some(reason.to_string());
        json!({"ok":true,"finished":true,"generated":context.session.generated()}).to_string()
    })
}

/// Responses 保留全部续传项，其他协议保存调用和结果的配对历史。
fn assistant_history(batch: &ToolCallBatch) -> Vec<ToolMessage> {
    if !batch.continuation_items.is_empty() {
        return batch
            .continuation_items
            .iter()
            .cloned()
            .map(|value| ToolMessage::ProviderItem { value })
            .collect();
    }
    batch
        .calls
        .iter()
        .map(|call| ToolMessage::AssistantCall {
            id: call.id.clone(),
            item_id: call.item_id.clone(),
            name: call.name.clone(),
            arguments: call.arguments.clone(),
        })
        .collect()
}

/// 错误只反馈当前调用的修正要求，不诱导模型重复成功卡片。
pub(super) fn error_result(code: &str, message: &str) -> String {
    let mut value = crate::ai::tools::spec::ToolResult::error(code, message).value;
    value["action"] = json!("retry");
    value["instruction"] = json!("仅修正并重试当前失败调用，不要重复已经成功的工具调用");
    value.to_string()
}

/// 只读取结构化布尔状态，正文里类似 JSON 的片段不能伪造成功。
fn result_ok(content: &str) -> bool {
    serde_json::from_str::<Value>(content)
        .ok()
        .and_then(|result| result["ok"].as_bool())
        == Some(true)
}

#[cfg(test)]
#[path = "generation_round_tests.rs"]
mod tests;
