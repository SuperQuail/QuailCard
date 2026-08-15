use sqlx::FromRow;

use super::{
    helpers::{cjk_tokenize, parse_json_list},
    now_timestamp, Database,
};
use crate::{
    error::CommandError,
    models::{AdoptCardsInput, CardInput, NoteCard},
};

#[derive(Debug, FromRow)]
struct NoteCardRow {
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
    position: i64,
    scheduler_phase: String,
    due_at: i64,
    interval_days: i64,
    total_reviews: i64,
    version: i64,
}

impl Database {
    /// 查询指定笔记的全部卡片及调度状态。
    pub async fn list_note_cards(&self, note_path: &str) -> Result<Vec<NoteCard>, CommandError> {
        let rows = sqlx::query_as::<_, NoteCardRow>(
            "SELECT c.id, c.note_path, c.source_ref, c.kind, c.front, c.back, c.detail,
                    c.example, c.aliases_json, c.rubric_json, c.position,
                    rs.scheduler_phase, rs.due_at, rs.interval_days, rs.total_reviews, rs.version
             FROM cards c JOIN review_states rs ON rs.card_id = c.id
             WHERE c.note_path = ?
             ORDER BY c.position",
        )
        .bind(note_path)
        .fetch_all(&self.pool)
        .await?;
        rows.into_iter().map(convert_note_card_row).collect()
    }

    /// 保存或更新单张卡片，并确保调度状态存在。
    pub async fn save_card(&self, input: CardInput) -> Result<NoteCard, CommandError> {
        validate_card_input(&input)?;
        let card_id = input
            .id
            .clone()
            .filter(|id| !id.trim().is_empty() && !id.starts_with("draft-"))
            .unwrap_or_else(|| uuid::Uuid::now_v7().to_string());
        let now = now_timestamp();
        let position = if input.id.is_some() {
            None
        } else {
            Some(next_card_position(&self.pool, &input.note_path).await?)
        };

        let mut transaction = self.pool.begin().await?;
        let aliases_json = serde_json::to_string(&input.aliases)
            .map_err(|error| CommandError::new("SERIALIZATION_ERROR", error.to_string()))?;
        let rubric_json = serde_json::to_string(&input.rubric)
            .map_err(|error| CommandError::new("SERIALIZATION_ERROR", error.to_string()))?;
        if let Some(position) = position {
            sqlx::query(
                "INSERT INTO cards (
                    id, note_path, source_ref, kind, front, back, detail, example,
                    aliases_json, rubric_json, position, created_at, updated_at
                 ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
            )
            .bind(&card_id)
            .bind(&input.note_path)
            .bind(input.source_ref.as_deref().unwrap_or_default())
            .bind(&input.kind)
            .bind(input.front.trim())
            .bind(input.back.trim())
            .bind(input.detail.as_deref().unwrap_or_default())
            .bind(input.example.as_deref().unwrap_or_default())
            .bind(&aliases_json)
            .bind(&rubric_json)
            .bind(position)
            .bind(now)
            .bind(now)
            .execute(&mut *transaction)
            .await?;
        } else {
            sqlx::query(
                "UPDATE cards SET
                    source_ref = ?, kind = ?, front = ?, back = ?, detail = ?,
                    example = ?, aliases_json = ?, rubric_json = ?, updated_at = ?
                 WHERE id = ?",
            )
            .bind(input.source_ref.as_deref().unwrap_or_default())
            .bind(&input.kind)
            .bind(input.front.trim())
            .bind(input.back.trim())
            .bind(input.detail.as_deref().unwrap_or_default())
            .bind(input.example.as_deref().unwrap_or_default())
            .bind(&aliases_json)
            .bind(&rubric_json)
            .bind(now)
            .bind(&card_id)
            .execute(&mut *transaction)
            .await?;
        }
        sqlx::query(
            "INSERT OR IGNORE INTO review_states (
                card_id, due_at, interval_days, repetitions, lapses, total_reviews,
                last_result, version, updated_at, stability, difficulty,
                last_review_at, scheduler_phase, learning_step
             ) VALUES (?, ?, 0, 0, 0, 0, NULL, 0, ?, NULL, 5.0, NULL, 'new', 0)",
        )
        .bind(&card_id)
        .bind(now)
        .bind(now)
        .execute(&mut *transaction)
        .await?;
        sync_card_fts(
            &mut transaction,
            &card_id,
            input.front.trim(),
            input.back.trim(),
        )
        .await?;
        transaction.commit().await?;

        let row = sqlx::query_as::<_, NoteCardRow>(
            "SELECT c.id, c.note_path, c.source_ref, c.kind, c.front, c.back, c.detail,
                    c.example, c.aliases_json, c.rubric_json, c.position,
                    rs.scheduler_phase, rs.due_at, rs.interval_days, rs.total_reviews, rs.version
             FROM cards c JOIN review_states rs ON rs.card_id = c.id
             WHERE c.id = ?",
        )
        .bind(&card_id)
        .fetch_one(&self.pool)
        .await?;
        convert_note_card_row(row)
    }

    /// 删除卡片：复习状态与历史由外键级联清理。
    pub async fn delete_card(&self, card_id: &str) -> Result<(), CommandError> {
        let result = sqlx::query("DELETE FROM cards WHERE id = ?")
            .bind(card_id)
            .execute(&self.pool)
            .await?;
        if result.rows_affected() == 0 {
            return Err(CommandError::new("CARD_NOT_FOUND", "卡片不存在"));
        }
        sqlx::query("DELETE FROM cards_fts WHERE card_id = ?")
            .bind(card_id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    /// 批量采纳 AI 拆卡草稿，全部写入后返回成功数量。
    pub async fn adopt_cards(&self, input: &AdoptCardsInput) -> Result<usize, CommandError> {
        let mut count = 0;
        for draft in &input.cards {
            let field = |key: &str| -> String {
                draft
                    .fields
                    .get(key)
                    .cloned()
                    .unwrap_or_default()
                    .trim()
                    .to_string()
            };
            let front = field("front");
            let back = field("back");
            if front.is_empty() || back.is_empty() {
                continue;
            }
            let aliases = split_list_field(&field("aliases"));
            let rubric = split_list_field(&field("rubric"));
            let card = CardInput {
                id: None,
                note_path: input.note_path.clone(),
                source_ref: Some(field("source").trim().to_string()),
                kind: input.kind.clone(),
                front,
                back,
                detail: Some(field("detail")),
                example: Some(field("example")),
                aliases,
                rubric,
            };
            self.save_card(card).await?;
            count += 1;
        }
        Ok(count)
    }
}

/// 同步卡片 FTS 条目。
async fn sync_card_fts(
    transaction: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    card_id: &str,
    front: &str,
    back: &str,
) -> Result<(), CommandError> {
    sqlx::query("DELETE FROM cards_fts WHERE card_id = ?")
        .bind(card_id)
        .execute(&mut **transaction)
        .await?;
    sqlx::query("INSERT INTO cards_fts (card_id, front, back) VALUES (?, ?, ?)")
        .bind(card_id)
        .bind(cjk_tokenize(front))
        .bind(cjk_tokenize(back))
        .execute(&mut **transaction)
        .await?;
    Ok(())
}

/// 校验单张卡片的通用约束。
fn validate_card_input(input: &CardInput) -> Result<(), CommandError> {
    if !matches!(input.kind.as_str(), "vocabulary" | "qa") {
        return Err(CommandError::validation("卡片类型无效"));
    }
    if input.note_path.trim().is_empty() || input.note_path.chars().count() > 512 {
        return Err(CommandError::validation("笔记路径无效"));
    }
    let front = input.front.trim();
    let back = input.back.trim();
    if front.is_empty() || front.chars().count() > 2000 {
        return Err(CommandError::validation("卡片正面长度必须为 1-2000 个字符"));
    }
    if back.is_empty() || back.chars().count() > 8000 {
        return Err(CommandError::validation("卡片背面长度必须为 1-8000 个字符"));
    }
    Ok(())
}

/// 将逗号或顿号分隔的字段拆成列表。
fn split_list_field(value: &str) -> Vec<String> {
    value
        .split(['、', ',', '，'])
        .map(str::trim)
        .filter(|item| !item.is_empty())
        .map(String::from)
        .collect()
}

/// 计算指定笔记中下一张卡片的插入位置。
async fn next_card_position(pool: &sqlx::SqlitePool, note_path: &str) -> Result<i64, CommandError> {
    let current =
        sqlx::query_scalar::<_, Option<i64>>("SELECT MAX(position) FROM cards WHERE note_path = ?")
            .bind(note_path)
            .fetch_one(pool)
            .await?;
    Ok(current.unwrap_or(-1) + 1)
}

/// 将数据库行转换为卡片面板模型。
fn convert_note_card_row(row: NoteCardRow) -> Result<NoteCard, CommandError> {
    Ok(NoteCard {
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
        position: row.position,
        scheduler_phase: row.scheduler_phase,
        due_at: row.due_at,
        interval_days: row.interval_days,
        total_reviews: row.total_reviews,
        version: row.version,
    })
}
