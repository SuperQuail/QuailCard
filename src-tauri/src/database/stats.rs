use super::{now_timestamp, Database};
use crate::{error::CommandError, models::StudyStats};

impl Database {
    /// 汇总全局到期数量、卡片总数与本周完成率。
    pub async fn get_study_stats(&self) -> Result<StudyStats, CommandError> {
        let weekly_completed_count = sqlx::query_scalar::<_, i64>(
            "SELECT COUNT(*) FROM review_records
             WHERE date(reviewed_at, 'unixepoch', 'localtime') >=
                   date('now', 'localtime', 'weekday 0', '-6 days')",
        )
        .fetch_one(&self.pool)
        .await?;
        let due_count = sqlx::query_scalar::<_, i64>(
            "SELECT COUNT(*)
             FROM cards c JOIN review_states rs ON rs.card_id = c.id
             WHERE rs.due_at <= ?",
        )
        .bind(now_timestamp())
        .fetch_one(&self.pool)
        .await?;
        let total_cards = sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM cards")
            .fetch_one(&self.pool)
            .await?;
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
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::{CardInput, ReviewRating, SubmitReviewInput};

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

    #[tokio::test]
    /// 全新数据库没有凭空产生的每周完成率。
    async fn empty_database_has_no_weekly_rate() {
        let database = Database::connect_memory()
            .await
            .expect("创建测试数据库失败");
        let stats = database.get_study_stats().await.expect("查询学习统计失败");
        assert_eq!(stats.weekly_completion_rate, None);
        assert_eq!(stats.weekly_completed_count, 0);
        assert_eq!(stats.due_count, 0);
    }

    #[tokio::test]
    /// 保存新卡后全局到期数增加，提交复习后完成率更新。
    async fn review_updates_stats() {
        let database = Database::connect_memory()
            .await
            .expect("创建测试数据库失败");
        database
            .upsert_note_index("测试/笔记.md", "", 1)
            .await
            .expect("写索引失败");
        let card = database.save_card(test_card()).await.expect("保存卡片失败");
        let before = database.get_study_stats().await.expect("查询统计失败");
        assert_eq!(before.due_count, 1);
        assert_eq!(before.total_cards, 1);
        database
            .submit_review(SubmitReviewInput {
                card_id: card.id,
                rating: ReviewRating::Good,
                expected_version: 0,
                idempotency_key: "stats-review".to_string(),
            })
            .await
            .expect("提交复习失败");
        let stats = database.get_study_stats().await.expect("查询统计失败");
        assert_eq!(stats.weekly_completed_count, 1);
        assert_eq!(stats.weekly_completion_rate, Some(100));
    }
}
