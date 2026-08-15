use std::collections::HashSet;

use crate::error::CommandError;

/// 从文件路径推导笔记标题。
pub(super) fn note_title_from_path(path: &str) -> String {
    path.rsplit(['/', '\\'])
        .next()
        .unwrap_or(path)
        .trim_end_matches(".md")
        .to_string()
}

/// 从正文提取行内标签。
pub(super) fn extract_tags(content: &str) -> Vec<String> {
    let mut seen = HashSet::new();
    let mut tags = Vec::new();
    for word in content.split_whitespace() {
        if let Some(tag) = word.strip_prefix('#') {
            let tag =
                tag.trim_matches(|c: char| !(c.is_alphanumeric() || matches!(c, '_' | '-' | '/')));
            if !tag.is_empty() && tag.chars().count() <= 50 && seen.insert(tag.to_string()) {
                tags.push(tag.to_string());
                if tags.len() >= 20 {
                    break;
                }
            }
        }
    }
    tags
}

/// 将中文连续字符用空格分隔，使 FTS 能按短语匹配中文子串。
pub(super) fn cjk_tokenize(text: &str) -> String {
    let mut result = String::with_capacity(text.len() + text.len() / 2);
    for character in text.chars() {
        if is_cjk(character) {
            result.push(' ');
            result.push(character);
            result.push(' ');
        } else {
            result.push(character);
        }
    }
    result.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// 判断字符是否为中日韩统一表意文字。
fn is_cjk(character: char) -> bool {
    matches!(character, '\u{4E00}'..='\u{9FFF}' | '\u{3400}'..='\u{4DBF}')
}

/// 在原文中定位查询词并截取带标记的片段。
pub(super) fn build_snippet(text: &str, query: &str) -> String {
    let query_chars: Vec<char> = query.trim().to_lowercase().chars().collect();
    let text_chars: Vec<char> = text.chars().collect();
    let lower_chars: Vec<char> = text.to_lowercase().chars().collect();
    if query_chars.is_empty() || text_chars.is_empty() {
        return String::new();
    }
    let mut index = 0;
    'search: while index + query_chars.len() <= lower_chars.len() {
        for offset in 0..query_chars.len() {
            if lower_chars[index + offset] != query_chars[offset] {
                index += 1;
                continue 'search;
            }
        }
        break;
    }
    let start = index.saturating_sub(20);
    let match_end = (index + query_chars.len()).min(text_chars.len());
    let end = (match_end + 30).min(text_chars.len());
    let mut snippet = String::new();
    if start > 0 {
        snippet.push('…');
    }
    snippet.extend(text_chars[start..index].iter());
    snippet.push_str("<mark>");
    snippet.extend(text_chars[index..match_end].iter());
    snippet.push_str("</mark>");
    snippet.extend(text_chars[match_end..end].iter());
    if end < text_chars.len() {
        snippet.push('…');
    }
    snippet
}

/// 规范化听写答案：全角折叠、小写、压缩空白。
pub(super) fn normalize_answer(value: &str) -> String {
    value
        .chars()
        .map(fold_fullwidth_char)
        .collect::<String>()
        .trim()
        .to_lowercase()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

/// 将常见全角字符折叠为半角，代替完整 NFKC。
fn fold_fullwidth_char(c: char) -> char {
    if ('\u{FF01}'..='\u{FF5E}').contains(&c) {
        char::from_u32(c as u32 - 0xFEE0).unwrap_or(c)
    } else if c == '\u{3000}' {
        ' '
    } else {
        c
    }
}

/// 解析数据库中的字符串数组 JSON。
pub(super) fn parse_json_list(value: &str) -> Result<Vec<String>, CommandError> {
    serde_json::from_str(value)
        .map_err(|error| CommandError::new("DATABASE_DATA_INVALID", error.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    /// 中文分词会保留单字检索能力。
    fn tokenizes_cjk_characters() {
        assert_eq!(cjk_tokenize("Rust 所有权"), "Rust 所 有 权");
        assert_eq!(cjk_tokenize("hello world"), "hello world");
    }

    #[test]
    /// 标签提取会去重并限制数量。
    fn extracts_unique_tags() {
        let tags = extract_tags("#rust #所有权 #rust 正文");
        assert_eq!(tags, ["rust", "所有权"]);
    }

    #[test]
    /// 听写答案规范化忽略大小写、空白和全角字符。
    fn normalizes_dictation_answers() {
        assert_eq!(normalize_answer("  ＥＰＨＥＭＥＲＡＬ "), "ephemeral");
    }
}
