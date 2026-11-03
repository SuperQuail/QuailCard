//! 单词组（.qword）文件的纯领域模型：解析、校验与卡片字段映射。
//!
//! 本模块不触碰文件系统：调用方读入文本后交给 [parse]，再把结果用于索引、
//! 卡片物化与保存。格式契约见已批准方案第三节：formatVersion 当前为 1，
//! 高于 1 拒绝（版本熔断）；读容忍（缺字段取默认、未知字段忽略）、写完整。

use std::collections::HashSet;

use serde::{Deserialize, Serialize};

use crate::error::CommandError;

/// 当前应用支持的单词组文件格式版本。
pub(crate) const FORMAT_VERSION: u64 = 1;

/// 单词组最多允许的页数：防止异常文件拖垮索引与卡片同步。
pub(crate) const MAX_PAGES: usize = 5000;

/// 页面 id 的最大字符数；id 即卡片 id，长度与字符集都必须受控。
pub(crate) const MAX_ID_CHARS: usize = 64;

/// 单词字段的最大字符数。
pub(crate) const MAX_WORD_CHARS: usize = 200;

/// 音标字段的最大字符数。
pub(crate) const MAX_PHONETIC_CHARS: usize = 200;

/// 释义字段的最大字符数。
pub(crate) const MAX_MEANING_CHARS: usize = 2000;

/// 例句字段的最大字符数。
pub(crate) const MAX_EXAMPLE_CHARS: usize = 8000;

/// 单词组文件的完整内容；words 的顺序即页面顺序。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub(crate) struct WordGroupDocument {
    /// 文件格式版本；缺失按当前版本读取。
    #[serde(default = "current_format_version")]
    pub(crate) format_version: u64,
    /// 全部页面；允许空数组（空组只显示提示，不产生卡片）。
    pub(crate) words: Vec<WordPage>,
}

impl Default for WordGroupDocument {
    /// 容错读取与新建共用的骨架：当前版本、无页面。
    fn default() -> Self {
        Self {
            format_version: FORMAT_VERSION,
            words: Vec::new(),
        }
    }
}

/// 单个单词页面；缺字段取默认、未知字段忽略。
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub(crate) struct WordPage {
    /// 页面与卡片的全局唯一标识：1–64 个 ASCII 字母/数字/-/_。
    pub(crate) id: String,
    /// 单词，卡片背面（听写答案）。
    pub(crate) word: String,
    /// 音标，卡片 detail。
    pub(crate) phonetic: String,
    /// 释义，卡片正面（提示）。
    pub(crate) meaning: String,
    /// 例句，卡片 example。
    pub(crate) example: String,
}

/// 完整页的卡片字段映射；字段含义与物化契约一一对应。
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct WordCardFields {
    /// 卡片正面：释义（给用户看的提示）。
    pub(crate) front: String,
    /// 卡片背面：单词（听写答案）。
    pub(crate) back: String,
    /// 卡片补充：音标。
    pub(crate) detail: String,
    /// 卡片例句。
    pub(crate) example: String,
}

/// 返回当前格式版本，供 serde 在字段缺失时取默认值。
fn current_format_version() -> u64 {
    FORMAT_VERSION
}

/// 解析并校验单词组文件内容。
///
/// 顺序：先按 JSON 值校验版本（熔断先于结构解析，避免把新格式误报成损坏），
/// 再反序列化（读容忍）并做页数、id、长度校验。错误信息只带页码，不带路径。
pub(crate) fn parse(content: &str) -> Result<WordGroupDocument, CommandError> {
    let value: serde_json::Value = serde_json::from_str(content).map_err(|error| {
        eprintln!("WORD_GROUP_PARSE_ERROR(detail): {error}");
        CommandError::new("WORD_GROUP_INVALID", "单词组文件不是有效的 JSON")
    })?;
    let version = match value.get("formatVersion") {
        None | Some(serde_json::Value::Null) => FORMAT_VERSION,
        Some(version) => version.as_u64().ok_or_else(|| {
            CommandError::new(
                "WORD_GROUP_INVALID",
                "单词组文件的 formatVersion 必须是整数",
            )
        })?,
    };
    if version > FORMAT_VERSION {
        return Err(CommandError::new(
            "WORD_GROUP_FORMAT_NEWER",
            "单词组文件由更新版本的 QuailCard 创建，请先升级应用",
        ));
    }
    let document: WordGroupDocument = serde_json::from_value(value).map_err(|error| {
        eprintln!("WORD_GROUP_DESERIALIZE_ERROR(detail): {error}");
        CommandError::new("WORD_GROUP_INVALID", "单词组内容格式不正确")
    })?;
    validate(&document)?;
    Ok(document)
}

/// 构造新建单词组的初始文档：1 个空页 + uuid v7 id。
pub(crate) fn initial_document() -> WordGroupDocument {
    WordGroupDocument {
        format_version: FORMAT_VERSION,
        words: vec![WordPage {
            id: uuid::Uuid::now_v7().to_string(),
            ..WordPage::default()
        }],
    }
}

/// 把文档序列化为磁盘格式：2 空格缩进、末尾换行（与前端序列化一致）。
pub(crate) fn serialize(document: &WordGroupDocument) -> Result<String, CommandError> {
    let mut text = serde_json::to_string_pretty(document).map_err(|error| {
        eprintln!("WORD_GROUP_SERIALIZE_ERROR(detail): {error}");
        CommandError::new("WORD_GROUP_INVALID", "单词组内容序列化失败")
    })?;
    text.push('\n');
    Ok(text)
}

/// 判断页面是否完整：单词与释义（去首尾空白后）都非空才算完整页。
///
/// 草稿页不生成卡片，但已有同 id 卡片必须原样保留，避免编辑中间态丢历史。
pub(crate) fn card_fields(page: &WordPage) -> Option<WordCardFields> {
    let word = page.word.trim();
    let meaning = page.meaning.trim();
    if word.is_empty() || meaning.is_empty() {
        return None;
    }
    Some(WordCardFields {
        front: meaning.to_string(),
        back: word.to_string(),
        detail: page.phonetic.trim().to_string(),
        example: page.example.trim().to_string(),
    })
}

/// 派生全文搜索文本：单词、释义与例句按页拼接；音标不参与搜索。
pub(crate) fn search_text(document: &WordGroupDocument) -> String {
    let mut text = String::new();
    for page in &document.words {
        for field in [&page.word, &page.meaning, &page.example] {
            let trimmed = field.trim();
            if trimmed.is_empty() {
                continue;
            }
            if !text.is_empty() {
                text.push('\n');
            }
            text.push_str(trimmed);
        }
    }
    text
}

/// 校验文档结构约束；全部错误码统一为 WORD_GROUP_INVALID。
fn validate(document: &WordGroupDocument) -> Result<(), CommandError> {
    if document.words.len() > MAX_PAGES {
        return Err(CommandError::new(
            "WORD_GROUP_INVALID",
            format!("单词组最多 {MAX_PAGES} 页"),
        ));
    }
    let mut seen: HashSet<&str> = HashSet::with_capacity(document.words.len());
    for (index, page) in document.words.iter().enumerate() {
        if !valid_id(&page.id) {
            return Err(page_error(index, "的 id 不合法"));
        }
        if !seen.insert(page.id.as_str()) {
            return Err(page_error(index, "的 id 与前面的页面重复"));
        }
        if page.word.chars().count() > MAX_WORD_CHARS {
            return Err(page_error(
                index,
                &format!("的单词超过 {MAX_WORD_CHARS} 个字符"),
            ));
        }
        if page.phonetic.chars().count() > MAX_PHONETIC_CHARS {
            return Err(page_error(
                index,
                &format!("的音标超过 {MAX_PHONETIC_CHARS} 个字符"),
            ));
        }
        if page.meaning.chars().count() > MAX_MEANING_CHARS {
            return Err(page_error(
                index,
                &format!("的释义超过 {MAX_MEANING_CHARS} 个字符"),
            ));
        }
        if page.example.chars().count() > MAX_EXAMPLE_CHARS {
            return Err(page_error(
                index,
                &format!("的例句超过 {MAX_EXAMPLE_CHARS} 个字符"),
            ));
        }
    }
    Ok(())
}

/// 校验页面 id：1–64 个 ASCII 字母、数字、连字符或下划线。
fn valid_id(id: &str) -> bool {
    let length = id.chars().count();
    (1..=MAX_ID_CHARS).contains(&length)
        && id
            .chars()
            .all(|character| character.is_ascii_alphanumeric() || matches!(character, '-' | '_'))
}

/// 构造带 1 起始页码的安全校验错误；只暴露页码，不暴露文件路径。
fn page_error(index: usize, reason: &str) -> CommandError {
    CommandError::new(
        "WORD_GROUP_INVALID",
        format!("单词组第 {} 页{reason}", index + 1),
    )
}

#[cfg(test)]
#[path = "word_group_tests.rs"]
mod tests;
