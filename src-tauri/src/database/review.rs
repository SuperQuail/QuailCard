use sqlx::FromRow;
use uuid::Uuid;

use super::{now_timestamp, Database};
use crate::{
    error::CommandError,
    models::{AiEvaluationContext, ReviewProgress, SubmitReviewInput},
    scheduler::{self, ReviewState},
};

#[derive(Debug, FromRow)]
struct ReviewStateRow {
    due_at: i64,
    repetitions: i64,
    lapses: i64,
    total_reviews: i64,
    version: i64,
    stability: Option<f64>,
    difficulty: f64,
    last_review_at: Option<i64>,
    scheduler_phase: String,
    learning_step: i64,
}

/// 将数据库行转换为调度器所需的领域状态快照。
fn to_scheduler_state(row: &ReviewStateRow) -> ReviewState {
    ReviewState {
        repetitions: row.repetitions,
        lapses: row.lapses,
        total_reviews: row.total_reviews,
        version: row.version,
        stability: row.stability,
        difficulty: row.difficulty,
        last_review_at: row.last_review_at,
        scheduler_phase: row.scheduler_phase.clone(),
        learning_step: row.learning_step,
    }
}

impl Database {
    /// 幂等提交评分并原子更新调度状态与历史记录。
    pub async fn submit_review(
        &self,
        input: SubmitReviewInput,
    ) -> Result<ReviewProgress, CommandError> {
        self.submit_review_with_evaluation(input, None).await
    }

    /// 幂等提交 AI 评分并在同一事务中保存判定内容。
    pub async fn submit_review_with_evaluation(
        &self,
        input: SubmitReviewInput,
        evaluation_json: Option<&str>,
    ) -> Result<ReviewProgress, CommandError> {
        validate_review_input(&input)?;
        if let Some(existing_card_id) =
            review_record_card_id(&self.pool, &input.idempotency_key).await?
        {
            if existing_card_id != input.card_id {
                return Err(CommandError::validation("幂等键已被其他卡片使用"));
            }
            return self.get_review_progress(&input.card_id).await;
        }

        let mut transaction = self.pool.begin().await?;
        let current = sqlx::query_as::<_, ReviewStateRow>(
            "SELECT due_at, repetitions, lapses, total_reviews, version,
                    stability, difficulty, last_review_at, scheduler_phase, learning_step
             FROM review_states WHERE card_id = ?",
        )
        .bind(&input.card_id)
        .fetch_optional(&mut *transaction)
        .await?
        .ok_or_else(|| CommandError::new("CARD_NOT_FOUND", "卡片不存在"))?;
        if current.version != input.expected_version {
            return Err(CommandError::new(
                "REVIEW_CONFLICT",
                "卡片状态已在其他会话更新，请刷新队列",
            ));
        }

        let now = now_timestamp();
        let state = to_scheduler_state(&current);
        let next = scheduler::calculate_next_state(&state, input.rating, now);
        let result = sqlx::query(
            "UPDATE review_states SET
                due_at = ?, interval_days = ?, repetitions = ?, lapses = ?,
                total_reviews = ?, last_result = ?, version = ?, updated_at = ?,
                stability = ?, difficulty = ?, last_review_at = ?,
                scheduler_phase = ?, learning_step = ?
             WHERE card_id = ? AND version = ?",
        )
        .bind(next.due_at)
        .bind(next.interval_days)
        .bind(next.repetitions)
        .bind(next.lapses)
        .bind(next.total_reviews)
        .bind(input.rating.as_str())
        .bind(next.version)
        .bind(now)
        .bind(next.stability)
        .bind(next.difficulty)
        .bind(now)
        .bind(&next.scheduler_phase)
        .bind(scheduler::next_learning_step(&state, input.rating))
        .bind(&input.card_id)
        .bind(input.expected_version)
        .execute(&mut *transaction)
        .await?;
        if result.rows_affected() == 0 {
            return Err(CommandError::new(
                "REVIEW_CONFLICT",
                "卡片状态已在其他会话更新，请刷新队列",
            ));
        }

        sqlx::query("UPDATE cards SET updated_at = ? WHERE id = ?")
            .bind(now)
            .bind(&input.card_id)
            .execute(&mut *transaction)
            .await?;
        sqlx::query(
            "INSERT INTO review_records (
                id, card_id, result, scheduled_due_at, reviewed_at,
                idempotency_key, ai_evaluation_json
             ) VALUES (?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(Uuid::now_v7().to_string())
        .bind(&input.card_id)
        .bind(input.rating.as_str())
        .bind(current.due_at)
        .bind(now)
        .bind(&input.idempotency_key)
        .bind(evaluation_json)
        .execute(&mut *transaction)
        .await?;
        transaction.commit().await?;
        Ok(next)
    }

    /// 获取卡片最新调度进度。
    pub async fn get_review_progress(&self, card_id: &str) -> Result<ReviewProgress, CommandError> {
        sqlx::query_as::<_, ReviewProgress>(
            "SELECT due_at, interval_days, repetitions, lapses, total_reviews, version,
                    scheduler_phase, stability, difficulty
             FROM review_states WHERE card_id = ?",
        )
        .bind(card_id)
        .fetch_optional(&self.pool)
        .await?
        .ok_or_else(|| CommandError::new("CARD_NOT_FOUND", "卡片不存在"))
    }

    /// 从数据库读取单题 AI 判定所需的权威上下文。
    pub async fn get_ai_evaluation_context(
        &self,
        card_id: &str,
    ) -> Result<AiEvaluationContext, CommandError> {
        let row = sqlx::query_as::<_, (String, String, String)>(
            "SELECT front, back, rubric_json FROM cards WHERE id = ?",
        )
        .bind(card_id)
        .fetch_optional(&self.pool)
        .await?
        .ok_or_else(|| CommandError::new("CARD_NOT_FOUND", "卡片不存在"))?;
        let rubric_points = serde_json::from_str(&row.2)
            .map_err(|_| CommandError::new("DATABASE_DATA_INVALID", "卡片判定要点格式无效"))?;
        Ok(AiEvaluationContext {
            question: row.0,
            reference_answer: row.1,
            rubric_points,
        })
    }

    /// 查询已经提交的 AI 判定 JSON，并检查幂等键归属。
    pub async fn get_recorded_ai_evaluation(
        &self,
        idempotency_key: &str,
        card_id: &str,
    ) -> Result<Option<String>, CommandError> {
        let record = sqlx::query_as::<_, (String, Option<String>)>(
            "SELECT card_id, ai_evaluation_json
             FROM review_records WHERE idempotency_key = ?",
        )
        .bind(idempotency_key)
        .fetch_optional(&self.pool)
        .await?;
        match record {
            Some((record_card_id, _)) if record_card_id != card_id => {
                Err(CommandError::validation("幂等键已被其他卡片使用"))
            }
            Some((_, None)) => Err(CommandError::validation("幂等键已用于普通复习")),
            Some((_, evaluation)) => Ok(evaluation),
            None => Ok(None),
        }
    }
}

/// 校验评分输入中的幂等键和卡片标识。
fn validate_review_input(input: &SubmitReviewInput) -> Result<(), CommandError> {
    if input.card_id.trim().is_empty()
        || input.idempotency_key.trim().is_empty()
        || input.idempotency_key.len() > 200
    {
        return Err(CommandError::validation("卡片 ID 和幂等键不能为空"));
    }
    Ok(())
}

/// 查询指定幂等键已经关联的卡片。
async fn review_record_card_id(
    pool: &sqlx::SqlitePool,
    idempotency_key: &str,
) -> Result<Option<String>, CommandError> {
    sqlx::query_scalar::<_, String>("SELECT card_id FROM review_records WHERE idempotency_key = ?")
        .bind(idempotency_key)
        .fetch_optional(pool)
        .await
        .map_err(CommandError::from)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::{CardInput, ReviewRating};

    /// 创建 AI 判定测试使用的卡片。
    fn ai_test_card() -> CardInput {
        CardInput {
            id: None,
            note_path: "测试/AI.md".to_string(),
            source_ref: None,
            kind: "qa".to_string(),
            front: "问题".to_string(),
            back: "答案".to_string(),
            detail: None,
            example: None,
            aliases: Vec::new(),
            rubric: vec!["要点一".to_string(), "要点二".to_string()],
        }
    }

    #[tokio::test]
    /// AI 判定内容与复习调度在同一次提交后均可恢复。
    async fn records_ai_evaluation_with_review() {
        let database = Database::connect_memory()
            .await
            .expect("创建测试数据库失败");
        database
            .upsert_note_index("测试/AI.md", "", 1)
            .await
            .expect("写索引失败");
        let card = database
            .save_card(ai_test_card())
            .await
            .expect("保存测试卡片失败");
        let evaluation = r#"{"isCorrect":true,"feedback":"正确","missingPoints":[],"suggestedAnswer":"","progress":null}"#;
        let progress = database
            .submit_review_with_evaluation(
                SubmitReviewInput {
                    card_id: card.id.clone(),
                    rating: ReviewRating::Good,
                    expected_version: 0,
                    idempotency_key: "ai-review-key".to_string(),
                },
                Some(evaluation),
            )
            .await
            .expect("提交 AI 判定失败");
        assert_eq!(progress.version, 1);
        assert_eq!(
            database
                .get_recorded_ai_evaluation("ai-review-key", &card.id)
                .await
                .expect("读取 AI 判定失败")
                .as_deref(),
            Some(evaluation)
        );
        assert_eq!(
            database
                .get_ai_evaluation_context(&card.id)
                .await
                .expect("读取 AI 上下文失败")
                .rubric_points,
            ["要点一", "要点二"]
        );
    }
}
