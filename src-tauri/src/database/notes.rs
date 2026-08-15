use super::{
    helpers::{cjk_tokenize, extract_tags, note_title_from_path},
    now_timestamp, Database,
};
use crate::{error::CommandError, models::NoteSummary};

impl Database {
    /// 以一次事务重建全部笔记索引与 FTS 内容。
    pub async fn rebuild_note_index(
        &self,
        files: &[(String, String, i64)],
    ) -> Result<(), CommandError> {
        let mut transaction = self.pool.begin().await?;
        sqlx::query("DELETE FROM note_index")
            .execute(&mut *transaction)
            .await?;
        sqlx::query("DELETE FROM note_fts")
            .execute(&mut *transaction)
            .await?;
        for (path, content, mtime) in files {
            upsert_note_index_tx(&mut transaction, path, content, *mtime).await?;
        }
        transaction.commit().await?;
        Ok(())
    }

    /// 新增或更新单篇笔记的索引缓存。
    pub async fn upsert_note_index(
        &self,
        path: &str,
        content: &str,
        mtime: i64,
    ) -> Result<(), CommandError> {
        let mut transaction = self.pool.begin().await?;
        upsert_note_index_tx(&mut transaction, path, content, mtime).await?;
        transaction.commit().await?;
        Ok(())
    }

    /// 查询全部笔记摘要及实时卡片数量。
    pub async fn list_notes(&self) -> Result<Vec<NoteSummary>, CommandError> {
        let now = now_timestamp();
        let rows = sqlx::query_as::<_, NoteSummary>(
            "SELECT ni.path, ni.title, ni.tags_json, ni.mtime,
                    COUNT(c.id) AS card_count,
                    COALESCE(SUM(CASE WHEN rs.due_at <= ? THEN 1 ELSE 0 END), 0) AS due_count
             FROM note_index ni
             LEFT JOIN cards c ON c.note_path = ni.path
             LEFT JOIN review_states rs ON rs.card_id = c.id
             GROUP BY ni.path
             ORDER BY ni.title",
        )
        .bind(now)
        .fetch_all(&self.pool)
        .await?;
        Ok(rows)
    }

    /// 重命名文件夹后同步其下笔记索引与卡片路径。
    pub async fn rename_note_paths(
        &self,
        old_prefix: &str,
        new_prefix: &str,
    ) -> Result<(), CommandError> {
        let mut transaction = self.pool.begin().await?;
        // 读取受影响路径，重建 note_fts 的行级路径映射
        let affected: Vec<String> =
            sqlx::query_scalar("SELECT path FROM note_index WHERE path = ? OR path LIKE ?")
                .bind(old_prefix)
                .bind(format!("{old_prefix}/%"))
                .fetch_all(&mut *transaction)
                .await?;
        for path in affected {
            let new_path = if path == old_prefix {
                new_prefix.to_string()
            } else {
                format!("{new_prefix}/{}", &path[old_prefix.len() + 1..])
            };
            sqlx::query("UPDATE note_index SET path = ? WHERE path = ?")
                .bind(&new_path)
                .bind(&path)
                .execute(&mut *transaction)
                .await?;
            sqlx::query("DELETE FROM note_fts WHERE path = ?")
                .bind(&path)
                .execute(&mut *transaction)
                .await?;
            sqlx::query(
                "INSERT INTO note_fts (path, title, body)
                 SELECT ?, title, body FROM note_index WHERE path = ?",
            )
            .bind(&new_path)
            .bind(&new_path)
            .execute(&mut *transaction)
            .await?;
            sqlx::query("UPDATE cards SET note_path = ? WHERE note_path = ?")
                .bind(&new_path)
                .bind(&path)
                .execute(&mut *transaction)
                .await?;
        }
        transaction.commit().await?;
        Ok(())
    }

    /// 删除笔记文件后清理其索引与全部卡片。
    pub async fn delete_note_paths(&self, path: &str) -> Result<(), CommandError> {
        let mut transaction = self.pool.begin().await?;
        let card_ids: Vec<String> = sqlx::query_scalar("SELECT id FROM cards WHERE note_path = ?")
            .bind(path)
            .fetch_all(&mut *transaction)
            .await?;
        for card_id in card_ids {
            sqlx::query("DELETE FROM cards_fts WHERE card_id = ?")
                .bind(&card_id)
                .execute(&mut *transaction)
                .await?;
        }
        sqlx::query("DELETE FROM cards WHERE note_path = ?")
            .bind(path)
            .execute(&mut *transaction)
            .await?;
        sqlx::query("DELETE FROM note_fts WHERE path = ?")
            .bind(path)
            .execute(&mut *transaction)
            .await?;
        sqlx::query("DELETE FROM note_index WHERE path = ?")
            .bind(path)
            .execute(&mut *transaction)
            .await?;
        transaction.commit().await?;
        Ok(())
    }

    /// 删除文件夹后清理其下全部笔记索引与卡片。
    pub async fn delete_folder_paths(&self, prefix: &str) -> Result<(), CommandError> {
        let paths: Vec<String> =
            sqlx::query_scalar("SELECT path FROM note_index WHERE path = ? OR path LIKE ?")
                .bind(prefix)
                .bind(format!("{prefix}/%"))
                .fetch_all(&self.pool)
                .await?;
        for path in paths {
            self.delete_note_paths(&path).await?;
        }
        Ok(())
    }
}

/// 在事务中写入单篇笔记索引与 FTS 条目。
async fn upsert_note_index_tx(
    transaction: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    path: &str,
    content: &str,
    mtime: i64,
) -> Result<(), CommandError> {
    let title = note_title_from_path(path);
    let tags = extract_tags(content);
    let tags_json = serde_json::to_string(&tags)
        .map_err(|error| CommandError::new("SERIALIZATION_ERROR", error.to_string()))?;
    sqlx::query("DELETE FROM note_fts WHERE path = ?")
        .bind(path)
        .execute(&mut **transaction)
        .await?;
    let tokenized = cjk_tokenize(content);
    sqlx::query("INSERT INTO note_fts (path, title, body) VALUES (?, ?, ?)")
        .bind(path)
        .bind(&title)
        .bind(&tokenized)
        .execute(&mut **transaction)
        .await?;
    sqlx::query(
        "INSERT INTO note_index (path, title, tags_json, body_fts, mtime, updated_at)
         VALUES (?, ?, ?, ?, ?, ?)
         ON CONFLICT(path) DO UPDATE SET
            title = excluded.title, tags_json = excluded.tags_json,
            body_fts = excluded.body_fts, mtime = excluded.mtime,
            updated_at = excluded.updated_at",
    )
    .bind(path)
    .bind(&title)
    .bind(&tags_json)
    .bind(content)
    .bind(mtime)
    .bind(now_timestamp())
    .execute(&mut **transaction)
    .await?;
    Ok(())
}
