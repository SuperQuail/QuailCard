use std::time::Duration;

use serde_json::Value;
use uuid::Uuid;

use super::AppServices;
use crate::{
    ai::{
        build_evaluation_prompt, debug_stage, evaluation_tool, parse_evaluation_response,
        MultiToolRequest, ToolCallBatch, ToolRequest,
    },
    error::CommandError,
    models::{
        AiEvaluationResult, EvaluateAnswerInput, ProviderConfig, ReviewRating, SubmitReviewInput,
    },
    storage::Storage,
};

impl AppServices {
    /// 独立判定当前回答，并原子写入判定、评分和调度结果。
    pub async fn evaluate_answer(
        &self,
        storage: &Storage,
        input: EvaluateAnswerInput,
    ) -> Result<AiEvaluationResult, CommandError> {
        validate_evaluation_input(&input)?;
        if !input.practice {
            if let Some(recorded) = storage
                .get_recorded_ai_evaluation(&input.idempotency_key, &input.card_id)
                .await?
            {
                return restore_recorded_evaluation(storage, &input.card_id, &recorded).await;
            }
        }

        let context = storage.get_ai_evaluation_context(&input.card_id).await?;
        let (system_prompt, user_prompt) = build_evaluation_prompt(&context, &input.user_answer)?;
        let tool = evaluation_tool();
        let config = storage.get_active_provider_config().await?;
        let trace_id = Uuid::now_v7().to_string();
        let arguments = self
            .call_configured_tool(
                storage,
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
        storage
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
        let recorded = storage
            .get_recorded_ai_evaluation(&input.idempotency_key, &input.card_id)
            .await?
            .ok_or_else(|| {
                CommandError::new("DATABASE_DATA_INVALID", "AI 判定记录提交后不可读取")
            })?;
        restore_recorded_evaluation(storage, &input.card_id, &recorded).await
    }

    /// 按供应商认证类型选择 API Key 或 ChatGPT OAuth 工具调用。
    async fn call_configured_tool(
        &self,
        storage: &Storage,
        config: &ProviderConfig,
        request: ToolRequest<'_>,
    ) -> Result<Value, CommandError> {
        match config.auth_type.as_deref() {
            Some("openai_oauth") => {
                let access = self.load_openai_access(storage, config).await?;
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
                let api_key = self.load_api_key(storage, config).await?;
                self.ai.call_tool(config, &api_key, request).await
            }
            _ => Err(CommandError::new(
                "PROVIDER_CREDENTIAL_MISSING",
                "请先配置 API Key 或登录 ChatGPT",
            )),
        }
    }

    /// 按供应商认证类型分发多工具自主调用请求。
    pub(super) async fn call_configured_multi_tool(
        &self,
        storage: &Storage,
        config: &ProviderConfig,
        request: MultiToolRequest<'_>,
    ) -> Result<ToolCallBatch, CommandError> {
        match config.auth_type.as_deref() {
            Some("openai_oauth") => {
                let access = self.load_openai_access(storage, config).await?;
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
                let api_key = self.load_api_key(storage, config).await?;
                self.ai.call_multi_tool(config, &api_key, request).await
            }
            _ => Err(CommandError::new(
                "PROVIDER_CREDENTIAL_MISSING",
                "请先配置 API Key 或登录 ChatGPT",
            )),
        }
    }
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
    storage: &Storage,
    card_id: &str,
    evaluation_json: &str,
) -> Result<AiEvaluationResult, CommandError> {
    let mut evaluation: AiEvaluationResult = serde_json::from_str(evaluation_json)
        .map_err(|_| CommandError::new("DATABASE_DATA_INVALID", "已保存的 AI 判定格式无效"))?;
    evaluation.progress = Some(storage.get_review_progress(card_id).await?);
    Ok(evaluation)
}
