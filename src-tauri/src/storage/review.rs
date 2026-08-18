//! 复习提交：幂等、乐观锁与调度更新，全部在卡片文件内原子完成。

use uuid::Uuid;

use super::{
    card_files::{lock_write, persist_cards, require_root},
    card_records::{HistoryRecord, ReviewStateRecord},
    now_timestamp, Storage,
};
use crate::{
    error::CommandError,
    models::{AiEvaluationContext, ReviewProgress, StudyStats, SubmitReviewInput},
    scheduler::{self, ReviewState},
};

/// 学习统计使用的滚动窗口：最近 7 天。
/// 原 SQLite 版按本地时区自然周统计；文件版改用滚动窗口避免引入时区依赖，
/// 两者的"本周完成率"语义差异仅体现在窗口边界上。
const WEEKLY_WINDOW_SECONDS: i64 = 7 * 86_400;

impl Storage {
    /// 幂等提交评分并更新调度状态与历史记录。
    pub async fn submit_review(
        &self,
        input: SubmitReviewInput,
    ) -> Result<ReviewProgress, CommandError> {
        self.submit_review_with_evaluation(input, None).await
    }

    /// 幂等提交 AI 评分并在同一文件中保存判定内容。
    pub async fn submit_review_with_evaluation(
        &self,
        input: SubmitReviewInput,
        evaluation_json: Option<&str>,
    ) -> Result<ReviewProgress, CommandError> {
        validate_review_input(&input)?;
        let mut state = lock_write(&self.inner.cards.state)?;
        let root = require_root(&state)?;
        // 幂等重放：同一幂等键返回当前进度，跨卡片复用直接拒绝。
        if let Some(recorded_card) = state.idempotency.get(&input.idempotency_key).cloned() {
            if recorded_card != input.card_id {
                return Err(CommandError::validation("幂等键已被其他卡片使用"));
            }
            return self.progress_of(&state, &input.card_id);
        }
        let Some(note_path) = state.card_note.get(&input.card_id).cloned() else {
            return Err(CommandError::new("CARD_NOT_FOUND", "卡片不存在"));
        };
        let progress = {
            let Some(cards) = state.notes.get_mut(&note_path) else {
                return Err(CommandError::new("CARD_NOT_FOUND", "卡片不存在"));
            };
            let Some(card) = cards.iter_mut().find(|card| card.id == input.card_id) else {
                return Err(CommandError::new("CARD_NOT_FOUND", "卡片不存在"));
            };
            if card.review.version != input.expected_version {
                return Err(CommandError::new(
                    "REVIEW_CONFLICT",
                    "卡片状态已在其他会话更新，请刷新队列",
                ));
            }
            let now = now_timestamp();
            let current = to_scheduler_state(&card.review);
            let progress = scheduler::calculate_next_state(&current, input.rating, now);
            let scheduled_due_at = card.review.due_at;
            card.review = ReviewStateRecord {
                due_at: progress.due_at,
                interval_days: progress.interval_days,
                repetitions: progress.repetitions,
                lapses: progress.lapses,
                total_reviews: progress.total_reviews,
                last_result: Some(input.rating.as_str().to_string()),
                version: progress.version,
                stability: progress.stability,
                difficulty: progress.difficulty,
                last_review_at: Some(now),
                scheduler_phase: progress.scheduler_phase.clone(),
                learning_step: scheduler::next_learning_step(&current, input.rating),
            };
            card.updated_at = now;
            card.history.push(HistoryRecord {
                id: Uuid::now_v7().to_string(),
                result: input.rating.as_str().to_string(),
                scheduled_due_at,
                reviewed_at: now,
                idempotency_key: input.idempotency_key.clone(),
                ai_evaluation: evaluation_json.and_then(|json| serde_json::from_str(json).ok()),
            });
            progress
        };
        state
            .idempotency
            .insert(input.idempotency_key.clone(), input.card_id.clone());
        let cards = state.notes.get(&note_path).cloned().unwrap_or_default();
        persist_cards(&root, &note_path, &cards)?;
        Ok(progress)
    }

    /// 获取卡片最新调度进度。
    pub async fn get_review_progress(&self, card_id: &str) -> Result<ReviewProgress, CommandError> {
        let state = self
            .inner
            .cards
            .state
            .read()
            .map_err(|_| CommandError::new("INTERNAL_ERROR", "卡片存储状态锁失效"))?;
        self.progress_of(&state, card_id)
    }

    /// 从读取锁内查询卡片进度。
    fn progress_of(
        &self,
        state: &super::cards::CardState,
        card_id: &str,
    ) -> Result<ReviewProgress, CommandError> {
        let note_path = state
            .card_note
            .get(card_id)
            .ok_or_else(|| CommandError::new("CARD_NOT_FOUND", "卡片不存在"))?;
        state
            .notes
            .get(note_path)
            .and_then(|cards| cards.iter().find(|card| card.id == card_id))
            .map(|card| card.to_progress())
            .ok_or_else(|| CommandError::new("CARD_NOT_FOUND", "卡片不存在"))
    }

    /// 读取单题 AI 判定所需的权威上下文。
    pub async fn get_ai_evaluation_context(
        &self,
        card_id: &str,
    ) -> Result<AiEvaluationContext, CommandError> {
        self.with_card(card_id, |card| AiEvaluationContext {
            question: card.front.clone(),
            reference_answer: card.back.clone(),
            rubric_points: card.rubric_points.clone(),
        })
        .await
    }

    /// 查询已经提交的 AI 判定 JSON，并检查幂等键归属。
    pub async fn get_recorded_ai_evaluation(
        &self,
        idempotency_key: &str,
        card_id: &str,
    ) -> Result<Option<String>, CommandError> {
        let state = self
            .inner
            .cards
            .state
            .read()
            .map_err(|_| CommandError::new("INTERNAL_ERROR", "卡片存储状态锁失效"))?;
        let Some(recorded_card) = state.idempotency.get(idempotency_key) else {
            return Ok(None);
        };
        if recorded_card != card_id {
            return Err(CommandError::validation("幂等键已被其他卡片使用"));
        }
        let note_path = state
            .card_note
            .get(card_id)
            .ok_or_else(|| CommandError::new("CARD_NOT_FOUND", "卡片不存在"))?;
        let evaluation = state
            .notes
            .get(note_path)
            .and_then(|cards| cards.iter().find(|card| card.id == card_id))
            .and_then(|card| {
                card.history
                    .iter()
                    .rev()
                    .find(|record| record.idempotency_key == idempotency_key)
            })
            .and_then(|record| record.ai_evaluation.clone());
        match evaluation {
            Some(value) => serde_json::to_string(&value)
                .map(Some)
                .map_err(|error| CommandError::new("SERIALIZATION_ERROR", error.to_string())),
            None => Err(CommandError::validation("幂等键已用于普通复习")),
        }
    }

    /// 汇总全局到期数量、卡片总数与最近 7 天完成量。
    pub async fn get_study_stats(&self) -> Result<StudyStats, CommandError> {
        let now = now_timestamp();
        let snapshot = self.inner.cards.snapshot_cards();
        let due_count = snapshot
            .iter()
            .filter(|(_, card)| card.review.due_at <= now)
            .count() as i64;
        let total_cards = snapshot.len() as i64;
        let weekly_completed_count = snapshot
            .iter()
            .flat_map(|(_, card)| card.history.iter())
            .filter(|record| record.reviewed_at >= now - WEEKLY_WINDOW_SECONDS)
            .count() as i64;
        let total = weekly_completed_count + due_count;
        let weekly_completion_rate = if total > 0 {
            Some((weekly_completed_count * 100 / total).clamp(0, 100))
        } else {
            None
        };
        Ok(StudyStats {
            due_count,
            total_cards,
            weekly_completion_rate,
            weekly_completed_count,
        })
    }

    /// 在读锁内访问单张卡片，统一卡片缺失错误。
    async fn with_card<T>(
        &self,
        card_id: &str,
        visitor: impl FnOnce(&super::card_records::CardRecord) -> T,
    ) -> Result<T, CommandError> {
        let state = self
            .inner
            .cards
            .state
            .read()
            .map_err(|_| CommandError::new("INTERNAL_ERROR", "卡片存储状态锁失效"))?;
        let note_path = state
            .card_note
            .get(card_id)
            .ok_or_else(|| CommandError::new("CARD_NOT_FOUND", "卡片不存在"))?;
        state
            .notes
            .get(note_path)
            .and_then(|cards| cards.iter().find(|card| card.id == card_id))
            .map(visitor)
            .ok_or_else(|| CommandError::new("CARD_NOT_FOUND", "卡片不存在"))
    }
}

/// 把持久化调度状态转换为调度器领域快照。
fn to_scheduler_state(record: &ReviewStateRecord) -> ReviewState {
    ReviewState {
        repetitions: record.repetitions,
        lapses: record.lapses,
        total_reviews: record.total_reviews,
        version: record.version,
        stability: record.stability,
        difficulty: record.difficulty,
        last_review_at: record.last_review_at,
        scheduler_phase: record.scheduler_phase.clone(),
        learning_step: record.learning_step,
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

#[cfg(test)]
mod tests {
    use super::super::testutil;
    use super::*;
    use crate::models::{CardInput, ReviewRating};

    /// 创建测试卡片输入。
    fn test_card() -> CardInput {
        CardInput {
            id: None,
            note_path: "测试/笔记.md".to_string(),
            source_ref: None,
            kind: "qa".to_string(),
            front: "问题".to_string(),
            back: "答案".to_string(),
            detail: None,
            example: None,
            aliases: Vec::new(),
            rubric: Vec::new(),
        }
    }

    /// 创建复习提交输入。
    fn review_input(card_id: &str, version: i64, key: &str) -> SubmitReviewInput {
        SubmitReviewInput {
            card_id: card_id.to_string(),
            rating: ReviewRating::Good,
            expected_version: version,
            idempotency_key: key.to_string(),
        }
    }

    #[tokio::test]
    /// AI 判定内容与复习调度在同一次提交后均可恢复。
    async fn records_ai_evaluation_with_review() {
        let (storage, _config, _vault) = testutil::test_storage().await;
        let card = storage
            .save_card(test_card())
            .await
            .expect("保存测试卡片失败");
        let evaluation = r#"{"isCorrect":true,"feedback":"正确","missingPoints":[],"suggestedAnswer":"","progress":null}"#;
        let progress = storage
            .submit_review_with_evaluation(
                review_input(&card.id, 0, "ai-review-key"),
                Some(evaluation),
            )
            .await
            .expect("提交 AI 判定失败");
        assert_eq!(progress.version, 1);
        // 判定原文以 JSON 值保存，往返会规范化键序，因此比较解析值而非字节。
        let recorded = storage
            .get_recorded_ai_evaluation("ai-review-key", &card.id)
            .await
            .expect("读取 AI 判定失败")
            .expect("判定记录应存在");
        let recorded_value: serde_json::Value =
            serde_json::from_str(&recorded).expect("判定应为有效 JSON");
        let expected_value: serde_json::Value =
            serde_json::from_str(evaluation).expect("判定原文应为有效 JSON");
        assert_eq!(recorded_value, expected_value);
        assert_eq!(
            storage
                .get_ai_evaluation_context(&card.id)
                .await
                .expect("读取 AI 上下文失败")
                .rubric_points,
            Vec::<String>::new()
        );
    }

    #[tokio::test]
    /// 同一幂等键重复提交返回当前进度且不追加历史。
    async fn idempotent_replay_returns_progress() {
        let (storage, _config, _vault) = testutil::test_storage().await;
        let card = storage.save_card(test_card()).await.expect("保存卡片失败");
        let first = storage
            .submit_review(review_input(&card.id, 0, "key-1"))
            .await
            .expect("首次提交失败");
        let replay = storage
            .submit_review(review_input(&card.id, 99, "key-1"))
            .await
            .expect("幂等重放失败");
        assert_eq!(first.version, replay.version);
    }

    #[tokio::test]
    /// 幂等键被其他卡片占用时直接拒绝。
    async fn rejects_key_reused_by_other_card() {
        let (storage, _config, _vault) = testutil::test_storage().await;
        let first = storage.save_card(test_card()).await.expect("保存卡片失败");
        let mut second = test_card();
        second.front = "另一题".to_string();
        let second = storage.save_card(second).await.expect("保存卡片失败");
        storage
            .submit_review(review_input(&first.id, 0, "shared-key"))
            .await
            .expect("首次提交失败");
        let error = storage
            .submit_review(review_input(&second.id, 0, "shared-key"))
            .await
            .expect_err("跨卡片复用应被拒绝");
        assert_eq!(error.code, "VALIDATION_ERROR");
    }

    #[tokio::test]
    /// 版本不匹配时返回复习冲突。
    async fn version_conflict_is_reported() {
        let (storage, _config, _vault) = testutil::test_storage().await;
        let card = storage.save_card(test_card()).await.expect("保存卡片失败");
        let error = storage
            .submit_review(review_input(&card.id, 7, "key-2"))
            .await
            .expect_err("过期版本应冲突");
        assert_eq!(error.code, "REVIEW_CONFLICT");
    }

    #[tokio::test]
    /// 全新存储没有凭空产生的完成率，复习后统计更新。
    async fn review_updates_stats() {
        let (storage, _config, _vault) = testutil::test_storage().await;
        let empty = storage.get_study_stats().await.expect("查询统计失败");
        assert_eq!(empty.weekly_completion_rate, None);
        assert_eq!(empty.due_count, 0);
        let card = storage.save_card(test_card()).await.expect("保存卡片失败");
        let before = storage.get_study_stats().await.expect("查询统计失败");
        assert_eq!(before.due_count, 1);
        assert_eq!(before.total_cards, 1);
        storage
            .submit_review(review_input(&card.id, 0, "stats-key"))
            .await
            .expect("提交复习失败");
        let stats = storage.get_study_stats().await.expect("查询统计失败");
        assert_eq!(stats.weekly_completed_count, 1);
        assert!(stats.weekly_completion_rate.is_some());
    }
}
