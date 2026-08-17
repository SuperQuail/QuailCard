use std::time::Duration;

use serde_json::{json, Value};
use uuid::Uuid;

use super::AppServices;
use crate::{
    ai::{
        build_generation_prompt, debug_stage, generation_tools, GenerationSession,
        MultiToolRequest, ToolArgumentError, ToolArguments, ToolCallBatch, ToolMessage,
    },
    database::Database,
    dictionary::{Dictionary, DictionaryEntry},
    error::CommandError,
    models::{GenerationInput, GenerationResult},
};

const MAX_DICTIONARY_RESULT_CHARS: usize = 6_000;
const MAX_GENERATION_TURNS: usize = 40;
const MAX_CORRECTIVE_ROUNDS: usize = 3;

struct RoundResult {
    history: Vec<ToolMessage>,
    corrective: bool,
    finish_auto: bool,
}

impl AppServices {
    /// 使用多轮单卡工具调用生成并累积可编辑卡片草稿。
    pub async fn generate_cards(
        &self,
        database: &Database,
        dictionary: &Dictionary,
        input: GenerationInput,
    ) -> Result<GenerationResult, CommandError> {
        let (system_prompt, user_prompt, max_tokens) = build_generation_prompt(&input)?;
        let tools = generation_tools(&input)?;
        let config = database.get_active_provider_config().await?;
        if !input.images.is_empty() && !config.supports_vision {
            return Err(CommandError::validation(
                "当前供应商未启用图片输入，请更换模型或在模型设置中开启",
            ));
        }
        let trace_id = Uuid::now_v7().to_string();
        let mut history = Vec::new();
        let mut session = GenerationSession::new(&input)?;
        let mut corrective_rounds = 0_usize;
        for turn in 1..=MAX_GENERATION_TURNS {
            debug_stage(&trace_id, format!("generation.turn: turn={turn}"));
            let batch = self
                .call_configured_multi_tool(
                    database,
                    &config,
                    MultiToolRequest {
                        trace_id: &trace_id,
                        turn,
                        system_prompt: &system_prompt,
                        user_prompt: &user_prompt,
                        images: &input.images,
                        tools: &tools,
                        history: &history,
                        max_tokens,
                        timeout: Duration::from_secs(120),
                    },
                )
                .await?;
            let round =
                process_generation_round(dictionary, &input, &mut session, batch, &trace_id)
                    .await?;
            history.extend(round.history);
            corrective_rounds = if round.corrective {
                corrective_rounds.saturating_add(1)
            } else {
                0
            };
            if session.fixed_complete() || round.finish_auto {
                debug_stage(
                    &trace_id,
                    format!("generation.done: cards={}", session.generated()),
                );
                return Ok(session.finish(None));
            }
            if corrective_rounds >= MAX_CORRECTIVE_ROUNDS {
                return finish_at_limit(session, "连续纠错轮次达到 3 次，已返回有效的部分结果");
            }
        }
        finish_at_limit(session, "生成轮次达到安全上限，已返回有效的部分结果")
    }
}

/// 独立处理一轮全部调用，确保失败调用不影响合法兄弟调用。
async fn process_generation_round(
    dictionary: &Dictionary,
    input: &GenerationInput,
    session: &mut GenerationSession,
    batch: ToolCallBatch,
    trace_id: &str,
) -> Result<RoundResult, CommandError> {
    let has_lookup = batch.calls.iter().any(|call| call.name == "lookup_words");
    let mut history = assistant_history(&batch);
    let mut results = vec![None; batch.calls.len()];
    let mut corrective = false;
    let mut finish_indexes = Vec::new();

    for (index, call) in batch.calls.iter().enumerate() {
        match call.name.as_str() {
            "lookup_words" => match &call.arguments {
                ToolArguments::Valid(arguments) if input.type_id == "vocabulary" => {
                    let content = resolve_lookup_result(dictionary, arguments).await?;
                    corrective |= result_code(&content) != "OK";
                    results[index] = Some(content);
                }
                ToolArguments::Invalid(error) => {
                    corrective = true;
                    results[index] = Some(invalid_json_result(error));
                }
                _ => {
                    corrective = true;
                    results[index] = Some(error_result(
                        "TOOL_NOT_AVAILABLE",
                        "当前卡片类型不能使用词典工具",
                    ));
                }
            },
            "emit_card" if has_lookup => {
                corrective = true;
                results[index] = Some(error_result(
                    "LOOKUP_RESULT_PENDING",
                    "同轮词典结果尚不可见，请依据工具结果重新调用 emit_card",
                ));
            }
            "emit_card" => match &call.arguments {
                ToolArguments::Valid(arguments) => match session.accept(input, arguments.clone()) {
                    Ok(()) => results[index] = Some(accepted_result(session)),
                    Err(error) => {
                        corrective = true;
                        results[index] = Some(error_result(error.code, &error.message));
                    }
                },
                ToolArguments::Invalid(error) => {
                    corrective = true;
                    results[index] = Some(invalid_json_result(error));
                }
            },
            "finish_generation" => finish_indexes.push(index),
            _ => {
                corrective = true;
                results[index] = Some(error_result("UNKNOWN_TOOL", "工具未注册，请改用可用工具"));
            }
        }
    }

    let mut finish_auto = false;
    for index in finish_indexes {
        let call = &batch.calls[index];
        let content = match &call.arguments {
            _ if input.requested_count != -1 => {
                corrective = true;
                error_result("TOOL_NOT_AVAILABLE", "固定数量生成不使用 finish_generation")
            }
            ToolArguments::Invalid(error) => {
                corrective = true;
                invalid_json_result(error)
            }
            ToolArguments::Valid(arguments) if !is_empty_object(arguments) => {
                corrective = true;
                error_result("INVALID_SCHEMA", "finish_generation 参数必须是空对象")
            }
            ToolArguments::Valid(_) if corrective => error_result(
                "CORRECTIONS_PENDING",
                "本轮仍有失败卡片，请修正后再次调用 finish_generation",
            ),
            ToolArguments::Valid(_) if session.can_finish_auto() => {
                finish_auto = true;
                json!({ "ok": true, "finished": true, "generated": session.generated() })
                    .to_string()
            }
            ToolArguments::Valid(_) => {
                corrective = true;
                error_result("NO_CARDS", "至少成功提交一张卡片后才能结束")
            }
        };
        results[index] = Some(content);
    }

    for (call, content) in batch.calls.into_iter().zip(results) {
        let content =
            content.unwrap_or_else(|| error_result("INTERNAL_RESULT_MISSING", "请重试此调用"));
        let code = result_code(&content);
        debug_stage(
            trace_id,
            format!(
                "tool.result: call={} tool={} code={code}",
                safe_log_token(&call.id),
                safe_log_token(&call.name)
            ),
        );
        history.push(ToolMessage::ToolResult {
            id: call.id,
            content,
        });
    }
    Ok(RoundResult {
        history,
        corrective,
        finish_auto,
    })
}

/// 保存一轮助手调用；Responses 使用已净化的原始续传项。
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

/// 构造单卡成功的结构化工具结果。
fn accepted_result(session: &GenerationSession) -> String {
    json!({
        "ok": true,
        "accepted": true,
        "generated": session.generated(),
        "remaining": session.remaining()
    })
    .to_string()
}

/// 构造要求模型重试的结构化安全错误。
fn error_result(code: &str, message: &str) -> String {
    json!({
        "ok": false,
        "error": { "code": code, "message": message },
        "action": "retry",
        "instruction": "仅修正并重试当前失败调用，不要重复已经成功的工具调用"
    })
    .to_string()
}

/// 将 JSON 解析位置转换为不含原始载荷的工具错误。
fn invalid_json_result(error: &ToolArgumentError) -> String {
    json!({
        "ok": false,
        "error": {
            "code": "INVALID_JSON",
            "message": "工具参数不是有效 JSON",
            "line": error.line,
            "column": error.column,
            "category": error.category
        },
        "action": "retry",
        "instruction": "根据错误位置重新生成当前调用的完整 JSON，不要重复已经成功的工具调用"
    })
    .to_string()
}

/// 净化供应商调用标识，避免控制字符和超长文本污染调试日志。
fn safe_log_token(value: &str) -> String {
    value
        .chars()
        .filter(|character| {
            character.is_ascii_alphanumeric() || matches!(character, '-' | '_' | '.')
        })
        .take(80)
        .collect()
}

/// 从结构化结果中读取日志使用的安全错误码。
fn result_code(content: &str) -> String {
    if content.contains("\"ok\":true") {
        "OK".to_string()
    } else {
        serde_json::from_str::<Value>(content)
            .ok()
            .and_then(|value| value.pointer("/error/code")?.as_str().map(str::to_owned))
            .unwrap_or_else(|| "UNKNOWN".to_string())
    }
}

/// 判断结束工具参数是否为严格空对象。
fn is_empty_object(arguments: &Value) -> bool {
    arguments.as_object().is_some_and(serde_json::Map::is_empty)
}

/// 达到轮次上限时返回部分结果，完全无结果则返回安全供应商错误。
fn finish_at_limit(
    session: GenerationSession,
    warning: &str,
) -> Result<GenerationResult, CommandError> {
    if session.generated() > 0 {
        Ok(session.finish(Some(warning.to_string())))
    } else {
        Err(CommandError::provider(
            "PROVIDER_GENERATION_LIMIT_REACHED",
            "模型多次未返回可用卡片，请稍后重试或更换模型",
        ))
    }
}

/// 执行词典查询并把结果序列化为工具结果文本。
async fn resolve_lookup_result(
    dictionary: &Dictionary,
    arguments: &Value,
) -> Result<String, CommandError> {
    let words = arguments
        .get("words")
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(Value::as_str)
                .map(str::trim)
                .filter(|word| !word.is_empty())
                .take(50)
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    if words.is_empty() {
        return Ok(error_result(
            "INVALID_SCHEMA",
            "words 必须包含至少一个有效单词",
        ));
    }
    let mut results = Vec::new();
    let mut truncated = false;
    for word in words {
        results.push(entry_to_json(word, dictionary.lookup(word).await?));
        let candidate = json!({ "ok": true, "results": results, "truncated": false }).to_string();
        if candidate.chars().count() > MAX_DICTIONARY_RESULT_CHARS {
            results.pop();
            truncated = true;
            break;
        }
    }
    serde_json::to_string(&json!({
        "ok": true,
        "results": results,
        "truncated": truncated
    }))
    .map_err(|error| CommandError::new("SERIALIZATION_ERROR", error.to_string()))
}

/// 将词典条目转换为查询结果 JSON，未命中单词也保留占位。
fn entry_to_json(word: &str, entry: Option<DictionaryEntry>) -> Value {
    match entry {
        Some(entry) => json!({
            "word": entry.word, "found": true, "phonetic": entry.phonetic,
            "translation": entry.translation, "definition": entry.definition,
            "pos": entry.pos, "collins": entry.collins, "oxford": entry.oxford,
            "bnc": entry.bnc, "frq": entry.frq, "exchange": entry.exchange
        }),
        None => json!({ "word": word, "found": false }),
    }
}

#[cfg(test)]
#[path = "ai_generation_tests.rs"]
mod tests;
