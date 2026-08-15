use sqlx::FromRow;

use super::{
    helpers::{build_snippet, cjk_tokenize, normalize_answer, parse_json_list},
    now_timestamp, Database,
};
use crate::{
    error::CommandError,
    models::{CardHit, DictationResult, NoteHit, ReviewCard, SearchResult},
};

#[derive(Debug, FromRow)]
struct ReviewCardRow {
    id: String,
    note_path: String,
    source_ref: String,
    kind: String,
    front: String,
    back: String,
    detail: String,
    example: String,
    aliases_json: String,
    rubric_json: String,
    scheduler_phase: String,
    version: i64,
}

impl Database {
    /// 读取复习队列：可限定笔记；include_all 为真时包含未到期卡片（自由练习）。
    pub async fn get_review_queue(
        &self,
        note_path: Option<&str>,
        include_all: bool,
    ) -> Result<Vec<ReviewCard>, CommandError> {
        let now = now_timestamp();
        let rows = sqlx::query_as::<_, ReviewCardRow>(
            "SELECT c.id, c.note_path, c.source_ref, c.kind, c.front, c.back, c.detail,
                    c.example, c.aliases_json, c.rubric_json, rs.scheduler_phase, rs.version
             FROM cards c JOIN review_states rs ON rs.card_id = c.id
             WHERE (? = '' OR c.note_path = ?) AND (? = 1 OR rs.due_at <= ?)
             ORDER BY CASE rs.scheduler_phase
                 WHEN 'relearning' THEN 0 WHEN 'learning' THEN 1
                 WHEN 'review' THEN 2 ELSE 3 END,
                 rs.due_at, c.note_path, c.position",
        )
        .bind(note_path.unwrap_or(""))
        .bind(note_path.unwrap_or(""))
        .bind(i64::from(include_all))
        .bind(now)
        .fetch_all(&self.pool)
        .await?;
        rows.into_iter().map(convert_review_card_row).collect()
    }

    /// 后端权威听写判定：规范化后与单词及别名比对。
    pub async fn check_dictation(
        &self,
        card_id: &str,
        answer: &str,
    ) -> Result<DictationResult, CommandError> {
        let row = sqlx::query_as::<_, (String, String)>(
            "SELECT back, aliases_json FROM cards WHERE id = ? AND kind = 'vocabulary'",
        )
        .bind(card_id)
        .fetch_optional(&self.pool)
        .await?
        .ok_or_else(|| CommandError::new("CARD_NOT_FOUND", "单词卡不存在"))?;
        let aliases: Vec<String> = serde_json::from_str(&row.1)
            .map_err(|_| CommandError::new("DATABASE_DATA_INVALID", "别名格式无效"))?;
        let normalized = normalize_answer(answer);
        let candidates = std::iter::once(row.0.as_str())
            .chain(aliases.iter().map(String::as_str))
            .map(normalize_answer);
        let correct = !normalized.is_empty() && candidates.clone().any(|value| value == normalized);
        Ok(DictationResult {
            correct,
            expected: row.0,
            aliases,
        })
    }

    /// 使用 FTS5 同时搜索笔记正文与卡片正反面。
    pub async fn search(&self, query: &str) -> Result<SearchResult, CommandError> {
        let keyword = query.trim();
        if keyword.is_empty() {
            return Ok(SearchResult {
                notes: Vec::new(),
                cards: Vec::new(),
            });
        }
        let match_query = format!("\"{}\"", cjk_tokenize(keyword).replace('"', " "));

        let note_paths = sqlx::query_scalar::<_, String>(
            "SELECT path FROM note_fts WHERE note_fts MATCH ? ORDER BY bm25(note_fts) LIMIT 20",
        )
        .bind(&match_query)
        .fetch_all(&self.pool)
        .await?;
        let mut notes = Vec::with_capacity(note_paths.len());
        for path in note_paths {
            let (title, body): (String, String) =
                sqlx::query_as("SELECT title, body_fts FROM note_index WHERE path = ?")
                    .bind(&path)
                    .fetch_optional(&self.pool)
                    .await?
                    .unwrap_or_default();
            notes.push(NoteHit {
                path,
                title,
                snippet: build_snippet(&body, keyword),
            });
        }

        let card_ids = sqlx::query_scalar::<_, String>(
            "SELECT card_id FROM cards_fts WHERE cards_fts MATCH ? ORDER BY bm25(cards_fts) LIMIT 20",
        )
        .bind(&match_query)
        .fetch_all(&self.pool)
        .await?;
        let mut cards = Vec::with_capacity(card_ids.len());
        for card_id in card_ids {
            let row = sqlx::query_as::<_, (String, String, String)>(
                "SELECT note_path, front, back FROM cards WHERE id = ?",
            )
            .bind(&card_id)
            .fetch_optional(&self.pool)
            .await?;
            if let Some((note_path, front, back)) = row {
                cards.push(CardHit {
                    card_id,
                    note_path,
                    front: front.clone(),
                    snippet: build_snippet(&format!("{front} {back}"), keyword),
                });
            }
        }

        Ok(SearchResult { notes, cards })
    }
}

/// 将数据库行转换为复习卡片模型。
fn convert_review_card_row(row: ReviewCardRow) -> Result<ReviewCard, CommandError> {
    Ok(ReviewCard {
        id: row.id,
        note_path: row.note_path,
        source_ref: row.source_ref,
        kind: row.kind,
        front: row.front,
        back: row.back,
        detail: row.detail,
        example: row.example,
        aliases: parse_json_list(&row.aliases_json)?,
        rubric_points: parse_json_list(&row.rubric_json)?,
        state: row.scheduler_phase,
        version: row.version,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::CardInput;

    /// 创建测试卡片的便捷输入。
    fn test_card(note_path: &str, kind: &str) -> CardInput {
        CardInput {
            id: None,
            note_path: note_path.to_string(),
            source_ref: None,
            kind: kind.to_string(),
            front: "问题".to_string(),
            back: "答案".to_string(),
            detail: None,
            example: None,
            aliases: Vec::new(),
            rubric: Vec::new(),
        }
    }

    #[tokio::test]
    /// 新卡片立即出现在复习队列中。
    async fn new_card_appears_in_queue() {
        let database = Database::connect_memory()
            .await
            .expect("创建测试数据库失败");
        database
            .upsert_note_index("测试/笔记.md", "#标签 正文", 1)
            .await
            .expect("写索引失败");
        database
            .save_card(test_card("测试/笔记.md", "qa"))
            .await
            .expect("保存卡片失败");
        let queue = database
            .get_review_queue(Some("测试/笔记.md"), false)
            .await
            .expect("查询队列失败");
        assert_eq!(queue.len(), 1);
        assert_eq!(queue[0].state, "new");
    }

    #[tokio::test]
    /// 搜索可以同时命中笔记正文与卡片背面。
    async fn search_hits_notes_and_cards() {
        let database = Database::connect_memory()
            .await
            .expect("创建测试数据库失败");
        database
            .upsert_note_index("测试/笔记.md", "Rust 所有权", 1)
            .await
            .expect("写索引失败");
        let mut card = test_card("测试/笔记.md", "qa");
        card.back = "所有权规则".to_string();
        database.save_card(card).await.expect("保存卡片失败");
        let result = database.search("所有权").await.expect("搜索失败");
        assert_eq!(result.notes.len(), 1);
        assert_eq!(result.cards.len(), 1);
    }

    #[tokio::test]
    /// 听写判定规范化后接受别名。
    async fn dictation_accepts_aliases() {
        let database = Database::connect_memory()
            .await
            .expect("创建测试数据库失败");
        database
            .upsert_note_index("英语/词.md", "", 1)
            .await
            .expect("写索引失败");
        let mut card = test_card("英语/词.md", "vocabulary");
        card.back = "Ephemeral".to_string();
        card.aliases = vec!["ephemeral".to_string()];
        let saved = database.save_card(card).await.expect("保存卡片失败");
        let result = database
            .check_dictation(&saved.id, "  EPHEMERAL ")
            .await
            .expect("听写判定失败");
        assert!(result.correct);
    }

    #[tokio::test]
    /// 删除卡片会清理卡片表。
    async fn delete_card_removes_row() {
        let database = Database::connect_memory()
            .await
            .expect("创建测试数据库失败");
        database
            .upsert_note_index("测试/笔记.md", "", 1)
            .await
            .expect("写索引失败");
        let saved = database
            .save_card(test_card("测试/笔记.md", "qa"))
            .await
            .expect("保存卡片失败");
        database.delete_card(&saved.id).await.expect("删除卡片失败");
        let queue = database
            .get_review_queue(Some("测试/笔记.md"), true)
            .await
            .expect("查询队列失败");
        assert!(queue.is_empty());
    }
}
