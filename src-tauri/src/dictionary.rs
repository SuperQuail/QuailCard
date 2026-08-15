use std::path::Path;

use serde::Serialize;
use sqlx::{sqlite::SqliteConnectOptions, sqlite::SqlitePoolOptions, FromRow, SqlitePool};

use crate::error::CommandError;

/// ECDICT 词典条目，字段与 stardict 表一一对应。
#[derive(Debug, Clone, Serialize, FromRow)]
#[serde(rename_all = "camelCase")]
pub struct DictionaryEntry {
    pub word: String,
    pub phonetic: Option<String>,
    pub translation: Option<String>,
    pub definition: Option<String>,
    pub pos: Option<String>,
    pub collins: Option<i64>,
    pub oxford: Option<i64>,
    pub bnc: Option<i64>,
    pub frq: Option<i64>,
    pub exchange: Option<String>,
}

/// 内置 ECDICT 英汉词典的只读查询器。
#[derive(Clone)]
pub struct Dictionary {
    pool: SqlitePool,
}

impl Dictionary {
    /// 以只读模式打开指定路径的词典数据库。
    pub async fn connect(path: &Path) -> Result<Self, CommandError> {
        let options = SqliteConnectOptions::new()
            .filename(path)
            .read_only(true)
            .journal_mode(sqlx::sqlite::SqliteJournalMode::Off);
        let pool = SqlitePoolOptions::new()
            .max_connections(2)
            .connect_with(options)
            .await?;
        Ok(Self { pool })
    }

    /// 精确查询单词，优先按原文匹配，其次按去掉标点后的形式匹配。
    pub async fn lookup(&self, word: &str) -> Result<Option<DictionaryEntry>, CommandError> {
        let word = word.trim();
        if word.is_empty() {
            return Ok(None);
        }
        if let Some(entry) = self.query_by_word(word).await? {
            return Ok(Some(entry));
        }
        self.query_by_stripped_word(word).await
    }

    /// 按原词精确查询（列使用 NOCASE 排序规则，天然大小写不敏感）。
    async fn query_by_word(&self, word: &str) -> Result<Option<DictionaryEntry>, CommandError> {
        let entry = sqlx::query_as::<_, DictionaryEntry>(
            "SELECT word, phonetic, translation, definition, pos,
                    collins, oxford, bnc, frq, exchange
             FROM stardict WHERE word = ?",
        )
        .bind(word)
        .fetch_optional(&self.pool)
        .await?;
        Ok(entry)
    }

    /// 按去掉标点并小写的词形查询，用于匹配带符号或变形的词条。
    async fn query_by_stripped_word(
        &self,
        word: &str,
    ) -> Result<Option<DictionaryEntry>, CommandError> {
        let stripped = word
            .chars()
            .filter(|character| character.is_alphanumeric())
            .collect::<String>()
            .to_lowercase();
        if stripped.is_empty() {
            return Ok(None);
        }
        let entry = sqlx::query_as::<_, DictionaryEntry>(
            "SELECT word, phonetic, translation, definition, pos,
                    collins, oxford, bnc, frq, exchange
             FROM stardict WHERE sw = ?",
        )
        .bind(stripped)
        .fetch_optional(&self.pool)
        .await?;
        Ok(entry)
    }
}

#[cfg(test)]
impl Dictionary {
    /// 创建带简化词条的内存词典实例，供各模块测试复用。
    pub(crate) async fn connect_memory_for_test() -> Dictionary {
        let options = SqliteConnectOptions::new().filename(":memory:");
        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect_with(options)
            .await
            .expect("创建内存词典失败");
        sqlx::query(
            "CREATE TABLE stardict (
                word TEXT PRIMARY KEY NOT NULL COLLATE NOCASE,
                sw TEXT NOT NULL,
                phonetic TEXT,
                translation TEXT,
                definition TEXT,
                pos TEXT,
                collins INTEGER,
                oxford INTEGER,
                bnc INTEGER,
                frq INTEGER,
                exchange TEXT
            )",
        )
        .execute(&pool)
        .await
        .expect("创建词典表失败");
        sqlx::query(
            "INSERT INTO stardict (word, sw, phonetic, translation, definition, bnc, frq)
             VALUES ('ephemeral', 'ephemeral', 'i''fem?r?l', 'a. 短暂的', 'lasting a very short time', 15628, 14116),
                    ('look up', 'lookup', NULL, '查阅', 'search for information', NULL, NULL)",
        )
        .execute(&pool)
        .await
        .expect("插入词条失败");
        Dictionary { pool }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    #[ignore = "依赖本地真实词典文件，仅在开发环境手动运行"]
    /// 验证真实 ECDICT 文件的只读连接与查询。
    async fn verify_real_dictionary_file() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("resources/ecdict.db");
        let dictionary = Dictionary::connect(&path).await.expect("连接真实词典失败");
        let entry = dictionary
            .lookup("ephemeral")
            .await
            .expect("查询失败")
            .expect("未找到词条");
        assert_eq!(entry.word, "ephemeral");
        assert!(entry.translation.is_some());
    }

    #[tokio::test]
    /// 大小写不敏感地查找到词条并返回完整字段。
    async fn lookup_finds_word_case_insensitively() {
        let dictionary = Dictionary::connect_memory_for_test().await;
        let entry = dictionary
            .lookup("EPHEMERAL")
            .await
            .expect("查询失败")
            .expect("未找到词条");
        assert_eq!(entry.word, "ephemeral");
        assert_eq!(entry.bnc, Some(15628));
        assert_eq!(entry.translation.as_deref(), Some("a. 短暂的"));
    }

    #[tokio::test]
    /// 带空格或标点的词通过去掉非字母数字的索引词匹配。
    async fn lookup_falls_back_to_stripped_word() {
        let dictionary = Dictionary::connect_memory_for_test().await;
        let entry = dictionary
            .lookup("look-up")
            .await
            .expect("查询失败")
            .expect("未找到词条");
        assert_eq!(entry.word, "look up");
        assert_eq!(entry.translation.as_deref(), Some("查阅"));
    }

    #[tokio::test]
    /// 词典中不存在的词返回空结果。
    async fn lookup_returns_none_for_missing_word() {
        let dictionary = Dictionary::connect_memory_for_test().await;
        let entry = dictionary
            .lookup("nonexistentwordxyz")
            .await
            .expect("查询失败");
        assert!(entry.is_none());
    }
}
