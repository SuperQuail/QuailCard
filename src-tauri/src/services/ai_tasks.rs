use std::time::Duration;

use serde_json::{json, Value};
use uuid::Uuid;

use super::AppServices;
use crate::{
    ai::{
        build_evaluation_prompt, build_generation_prompt, debug_stage, evaluation_tool,
        generation_tools, parse_evaluation_response, parse_generation_response, MultiToolRequest,
        ToolCallBatch, ToolMessage, ToolRequest,
    },
    database::Database,
    dictionary::{Dictionary, DictionaryEntry},
    error::CommandError,
    models::{
        AiEvaluationResult, EvaluateAnswerInput, GenerationInput, GenerationResult, ProviderConfig,
        ReviewRating, SubmitReviewInput,
    },
};

/// 词典工具调用后注入模型上下文的最大字符数。
const MAX_DICTIONARY_RESULT_CHARS: usize = 6_000;

impl AppServices {
    /// 使用当前活动供应商将文本材料转换为可编辑卡片草稿。
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
        let mut history = Vec::new();
        let trace_id = Uuid::now_v7().to_string();
        let mut turn = 1_usize;
        loop {
            debug_stage(
                &trace_id,
                format!(
                    "generation.turn.start: turn={turn} history={}",
                    history.len()
                ),
            );
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
            for call in &batch.calls {
                if !matches!(call.name.as_str(), "lookup_words" | "emit_cards") {
                    return Err(CommandError::provider(
                        "PROVIDER_TOOL_RESPONSE_INVALID",
                        format!("模型调用了未注册的工具 {}", call.name),
                    ));
                }
            }
            let looked_up_words = batch.calls.iter().any(|call| call.name == "lookup_words");
            let emitted_cards = batch
                .calls
                .iter()
                .find(|call| call.name == "emit_cards")
                .map(|call| call.arguments.clone());
            if looked_up_words {
                if batch.continuation_items.is_empty() {
                    for call in &batch.calls {
                        history.push(ToolMessage::AssistantCall {
                            id: call.id.clone(),
                            item_id: call.item_id.clone(),
                            name: call.name.clone(),
                            arguments: call.arguments.clone(),
                        });
                    }
                } else {
                    for item in batch.continuation_items {
                        history.push(ToolMessage::ProviderItem { value: item });
                    }
                }
                for call in batch.calls {
                    if call.name == "lookup_words" {
                        let word_count = lookup_word_count(&call.arguments);
                        debug_stage(
                            &trace_id,
                            format!("tool.execute: lookup_words words={word_count}"),
                        );
                        let content = resolve_lookup_result(dictionary, &call.arguments).await?;
                        debug_stage(
                            &trace_id,
                            format!(
                                "tool.result: lookup_words chars={}",
                                content.chars().count()
                            ),
                        );
                        history.push(ToolMessage::ToolResult {
                            id: call.id,
                            content,
                        });
                    } else {
                        history.push(ToolMessage::ToolResult {
                            id: call.id,
                            content: deferred_emit_result(),
                        });
                    }
                }
                if emitted_cards.is_some() {
                    debug_stage(
                        &trace_id,
                        "generation.defer: 同轮词典查询完成，忽略尚未使用查询结果的 emit_cards",
                    );
                }
                turn = turn.saturating_add(1);
                continue;
            }
            if let Some(arguments) = emitted_cards {
                let result = parse_generation_response(&input, arguments)?;
                debug_stage(
                    &trace_id,
                    format!("generation.done: cards={}", result.cards.len()),
                );
                return Ok(result);
            }
        }
    }

    /// 独立判定当前回答，并原子写入判定、评分和调度结果。
    pub async fn evaluate_answer(
        &self,
        database: &Database,
        input: EvaluateAnswerInput,
    ) -> Result<AiEvaluationResult, CommandError> {
        validate_evaluation_input(&input)?;
        if !input.practice {
            if let Some(recorded) = database
                .get_recorded_ai_evaluation(&input.idempotency_key, &input.card_id)
                .await?
            {
                return restore_recorded_evaluation(database, &input.card_id, &recorded).await;
            }
        }

        let context = database.get_ai_evaluation_context(&input.card_id).await?;
        let (system_prompt, user_prompt) = build_evaluation_prompt(&context, &input.user_answer)?;
        let tool = evaluation_tool();
        let config = database.get_active_provider_config().await?;
        let trace_id = Uuid::now_v7().to_string();
        let arguments = self
            .call_configured_tool(
                database,
                &config,
                ToolRequest {
                    trace_id: &trace_id,
                    turn: 1,
                    system_prompt: &system_prompt,
                    user_prompt: &user_prompt,
                    images: &[],
                    tool: &tool,
                    max_tokens: 1_200,
                    timeout: Duration::from_secs(45),
                },
            )
            .await?;
        let evaluation = parse_evaluation_response(arguments)?;
        debug_stage(
            &trace_id,
            format!("evaluation.done: correct={}", evaluation.is_correct),
        );
        if input.practice {
            return Ok(evaluation);
        }
        let evaluation_json = serde_json::to_string(&evaluation)
            .map_err(|error| CommandError::new("SERIALIZATION_ERROR", error.to_string()))?;
        database
            .submit_review_with_evaluation(
                SubmitReviewInput {
                    card_id: input.card_id.clone(),
                    rating: if evaluation.is_correct {
                        ReviewRating::Good
                    } else {
                        ReviewRating::Again
                    },
                    expected_version: input.expected_version,
                    idempotency_key: input.idempotency_key.clone(),
                },
                Some(&evaluation_json),
            )
            .await?;
        let recorded = database
            .get_recorded_ai_evaluation(&input.idempotency_key, &input.card_id)
            .await?
            .ok_or_else(|| {
                CommandError::new("DATABASE_DATA_INVALID", "AI 判定记录提交后不可读取")
            })?;
        restore_recorded_evaluation(database, &input.card_id, &recorded).await
    }

    /// 按供应商认证类型选择 API Key 或 ChatGPT OAuth 工具调用。
    async fn call_configured_tool(
        &self,
        database: &Database,
        config: &ProviderConfig,
        request: ToolRequest<'_>,
    ) -> Result<Value, CommandError> {
        match config.auth_type.as_deref() {
            Some("openai_oauth") => {
                let access = self.load_openai_access(database, config).await?;
                self.ai
                    .call_openai_oauth_tool(
                        config,
                        &access.access_token,
                        access.account_id.as_deref(),
                        request,
                    )
                    .await
            }
            Some("api_key") => {
                let api_key = self.load_api_key(database, config).await?;
                self.ai.call_tool(config, &api_key, request).await
            }
            _ => Err(CommandError::new(
                "PROVIDER_CREDENTIAL_MISSING",
                "请先配置 API Key 或登录 ChatGPT",
            )),
        }
    }

    /// 按供应商认证类型分发多工具自主调用请求。
    async fn call_configured_multi_tool(
        &self,
        database: &Database,
        config: &ProviderConfig,
        request: MultiToolRequest<'_>,
    ) -> Result<ToolCallBatch, CommandError> {
        match config.auth_type.as_deref() {
            Some("openai_oauth") => {
                let access = self.load_openai_access(database, config).await?;
                self.ai
                    .call_openai_oauth_multi_tool(
                        config,
                        &access.access_token,
                        access.account_id.as_deref(),
                        request,
                    )
                    .await
            }
            Some("api_key") => {
                let api_key = self.load_api_key(database, config).await?;
                self.ai.call_multi_tool(config, &api_key, request).await
            }
            _ => Err(CommandError::new(
                "PROVIDER_CREDENTIAL_MISSING",
                "请先配置 API Key 或登录 ChatGPT",
            )),
        }
    }
}

/// 返回同轮 emit_cards 被延后时写回模型的工具结果。
fn deferred_emit_result() -> String {
    json!({
        "deferred": true,
        "reason": "lookup_words results were not available when emit_cards was called; call emit_cards again using the tool results"
    })
    .to_string()
}

/// 读取 lookup_words 参数中有效单词的数量，供实时阶段日志使用。
fn lookup_word_count(arguments: &Value) -> usize {
    arguments
        .get("words")
        .and_then(Value::as_array)
        .map(|words| {
            words
                .iter()
                .filter_map(Value::as_str)
                .filter(|word| !word.trim().is_empty())
                .count()
        })
        .unwrap_or(0)
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
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    let mut results = Vec::new();
    for word in words.into_iter().take(50) {
        let entry = dictionary.lookup(word).await?;
        results.push(entry_to_json(word, entry));
    }
    let text = serde_json::to_string(&json!({ "results": results }))
        .map_err(|error| CommandError::new("SERIALIZATION_ERROR", error.to_string()))?;
    Ok(truncate_dictionary_result(text))
}

/// 将词典条目转换为查询结果 JSON，未命中的单词也保留占位。
fn entry_to_json(word: &str, entry: Option<DictionaryEntry>) -> Value {
    match entry {
        Some(entry) => json!({
            "word": entry.word,
            "found": true,
            "phonetic": entry.phonetic,
            "translation": entry.translation,
            "definition": entry.definition,
            "pos": entry.pos,
            "collins": entry.collins,
            "oxford": entry.oxford,
            "bnc": entry.bnc,
            "frq": entry.frq,
            "exchange": entry.exchange
        }),
        None => json!({ "word": word, "found": false }),
    }
}

/// 截断过长的词典结果，避免超出模型上下文。
fn truncate_dictionary_result(mut text: String) -> String {
    let max = MAX_DICTIONARY_RESULT_CHARS;
    if text.chars().count() > max {
        let truncated = text.chars().take(max).collect::<String>();
        text = format!("{truncated}…(结果已截断)");
    }
    text
}

/// 校验 AI 判定请求中的卡片、回答和幂等字段。
fn validate_evaluation_input(input: &EvaluateAnswerInput) -> Result<(), CommandError> {
    if input.card_id.trim().is_empty() {
        return Err(CommandError::validation("卡片 ID 无效"));
    }
    if !input.practice
        && (input.idempotency_key.trim().is_empty() || input.idempotency_key.len() > 200)
    {
        return Err(CommandError::validation("卡片 ID 或幂等键无效"));
    }
    if input.user_answer.trim().is_empty() || input.user_answer.chars().count() > 8_000 {
        return Err(CommandError::validation("回答长度必须为 1-8,000 个字符"));
    }
    Ok(())
}

/// 恢复幂等重试对应的历史判定和当前调度状态。
async fn restore_recorded_evaluation(
    database: &Database,
    card_id: &str,
    evaluation_json: &str,
) -> Result<AiEvaluationResult, CommandError> {
    let mut evaluation: AiEvaluationResult = serde_json::from_str(evaluation_json)
        .map_err(|_| CommandError::new("DATABASE_DATA_INVALID", "已保存的 AI 判定格式无效"))?;
    evaluation.progress = Some(database.get_review_progress(card_id).await?);
    Ok(evaluation)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    /// 词典工具结果包含命中与未命中单词的占位信息。
    async fn resolves_dictionary_lookup_result() {
        let dictionary = crate::dictionary::Dictionary::connect_memory_for_test().await;
        let result = resolve_lookup_result(
            &dictionary,
            &json!({ "words": ["ephemeral", "missingword"] }),
        )
        .await
        .expect("解析词典结果失败");
        let parsed: Value = serde_json::from_str(&result).expect("结果不是 JSON");
        assert_eq!(parsed["results"][0]["found"], true);
        assert_eq!(parsed["results"][1]["found"], false);
    }

    #[test]
    /// 同轮 emit_cards 被延后时会获得完整工具结果，允许下一轮继续对话。
    fn builds_deferred_emit_result() {
        let result: Value =
            serde_json::from_str(&deferred_emit_result()).expect("延后结果不是 JSON");
        assert_eq!(result["deferred"], true);
        assert!(result["reason"]
            .as_str()
            .is_some_and(|value| value.contains("emit_cards")));
    }
}
