//! 内置 ECDICT 英汉词典的只读查询。
//!
//! ecdict.db 是随应用打包的第三方词典资源（SQLite 文件格式），
//! 通过 rusqlite 以只读连接访问；它不承载任何用户数据，也不参与
//! 应用自身的文件存储体系。

use std::{
    path::Path,
    sync::{Arc, Mutex},
};

use rusqlite::{Connection, OpenFlags, OptionalExtension};
use serde::Serialize;

#[cfg(test)]
use uuid::Uuid;

use crate::error::CommandError;

/// ECDICT 词典条目，字段与 stardict 表一一对应。
#[derive(Debug, Clone, Serialize)]
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
    connection: Arc<Mutex<Connection>>,
}

/// 查询单个词条的 SQL 前缀，绑定词形作为唯一参数。
const LOOKUP_SQL: &str = "SELECT word, phonetic, translation, definition, pos,
        collins, oxford, bnc, frq, exchange
 FROM stardict WHERE ";

impl Dictionary {
    /// 以只读模式打开指定路径的词典数据库。
    pub fn connect(path: &Path) -> Result<Self, CommandError> {
        let flags = OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX;
        let connection = Connection::open_with_flags(path, flags).map_err(|error| {
            eprintln!("DICTIONARY_ERROR(detail): {error}");
            CommandError::new("DICTIONARY_ERROR", "词典数据库加载失败")
        })?;
        Ok(Self {
            connection: Arc::new(Mutex::new(connection)),
        })
    }

    /// 精确查询单词，优先按原文匹配，其次按去掉标点后的形式匹配。
    ///
    /// SQLite 查询是阻塞操作，统一放到阻塞线程池执行，避免卡住异步运行时。
    pub async fn lookup(&self, word: &str) -> Result<Option<DictionaryEntry>, CommandError> {
        let word = word.trim().to_string();
        if word.is_empty() {
            return Ok(None);
        }
        let connection = self.connection.clone();
        tokio::task::spawn_blocking(move || {
            let guard = connection
                .lock()
                .map_err(|_| CommandError::new("DICTIONARY_ERROR", "词典查询失败"))?;
            if let Some(entry) = query_by_column(&guard, "word", &word)? {
                return Ok(Some(entry));
            }
            query_by_stripped_word(&guard, &word)
        })
        .await
        .map_err(|error| {
            eprintln!("DICTIONARY_JOIN_ERROR(detail): {error}");
            CommandError::new("DICTIONARY_ERROR", "词典查询失败")
        })?
    }
}

/// 按指定列的等值条件查询单个词条。
fn query_by_column(
    connection: &Connection,
    column: &str,
    value: &str,
) -> Result<Option<DictionaryEntry>, CommandError> {
    let sql = format!("{LOOKUP_SQL}{column} = ?1");
    let entry = connection
        .query_row(&sql, [value], map_entry)
        .optional()
        .map_err(|error| {
            eprintln!("DICTIONARY_QUERY_ERROR(detail): {error}");
            CommandError::new("DICTIONARY_ERROR", "词典查询失败")
        })?;
    Ok(entry)
}

/// 按去掉标点并小写的索引词（sw 列）查询。
fn query_by_stripped_word(
    connection: &Connection,
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
    query_by_column(connection, "sw", &stripped)
}

/// 把单行查询结果映射为词典条目。
fn map_entry(row: &rusqlite::Row<'_>) -> rusqlite::Result<DictionaryEntry> {
    Ok(DictionaryEntry {
        word: row.get(0)?,
        phonetic: row.get(1)?,
        translation: row.get(2)?,
        definition: row.get(3)?,
        pos: row.get(4)?,
        collins: row.get(5)?,
        oxford: row.get(6)?,
        bnc: row.get(7)?,
        frq: row.get(8)?,
        exchange: row.get(9)?,
    })
}

#[cfg(test)]
impl Dictionary {
    /// 创建带简化词条的临时文件词典实例，供各模块测试复用。
    ///
    /// 保留旧名称以最小化调用方改动；rusqlite 无法用内存库跨连接共享，
    /// 改为临时文件实现。
    pub(crate) async fn connect_memory_for_test() -> Dictionary {
        let path = std::env::temp_dir().join(format!("qc-dict-{}.db", Uuid::now_v7()));
        let connection = Connection::open(&path).expect("创建临时词典失败");
        connection
            .execute_batch(
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
                );
                INSERT INTO stardict (word, sw, phonetic, translation, definition, bnc, frq)
                 VALUES ('ephemeral', 'ephemeral', 'i''fem?r?l', 'a. 短暂的',
                         'lasting a very short time', 15628, 14116),
                        ('look up', 'lookup', NULL, '查阅',
                         'search for information', NULL, NULL);",
            )
            .expect("初始化词典数据失败");
        drop(connection);
        Dictionary::connect(&path).expect("打开临时词典失败")
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
        let dictionary = Dictionary::connect(&path).expect("连接真实词典失败");
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
