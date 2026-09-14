//! 生成与采纳共用的纯领域规则，不依赖文件、网络或 Tauri。

use sha2::{Digest, Sha256};

use crate::models::CardSource;

/// 统一全角、大小写和连续空白，避免同词或同问题换答案绕过判重。
fn normalize_identity(value: &str) -> String {
    value
        .chars()
        .map(|c| match c {
            '\u{FF01}'..='\u{FF5E}' => char::from_u32(c as u32 - 0xFEE0).unwrap_or(c),
            '\u{3000}' => ' ',
            _ => c,
        })
        .collect::<String>()
        .to_lowercase()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

/// 类型规则单点注册，生成与采纳必须使用同一身份字段。
pub(crate) fn card_identity(kind: &str, front: &str, back: &str) -> String {
    const ANSWER_IDENTITY_TYPES: &[&str] = &["vocabulary"];
    let value = if ANSWER_IDENTITY_TYPES.contains(&kind) {
        back
    } else {
        front
    };
    format!("{kind}\0{}", normalize_identity(value))
}

/// 只归一化写盘使用的换行，不改变 Markdown 空白与内容。
pub(crate) fn note_content_hash(content: &str) -> String {
    format!(
        "{:x}",
        Sha256::digest(content.replace("\r\n", "\n").as_bytes())
    )
}

/// UTF-16 偏移必须落在完整字符边界，防止 emoji 被截成代理项。
fn byte_offset(document: &str, utf16_offset: usize) -> Option<usize> {
    let mut offset = 0;
    for (index, c) in document.char_indices() {
        if offset == utf16_offset {
            return Some(index);
        }
        offset += c.len_utf16();
        if offset > utf16_offset {
            return None;
        }
    }
    (offset == utf16_offset).then_some(document.len())
}

/// 只有原坐标、摘录及两侧上下文都吻合时来源仍然有效。
pub(crate) fn validate_source(document: &str, source: &CardSource) -> bool {
    if source.to <= source.from {
        return false;
    }
    let Some(from) = byte_offset(document, source.from) else {
        return false;
    };
    let Some(to) = byte_offset(document, source.to) else {
        return false;
    };
    document[from..to] == source.excerpt
        && document[..from].ends_with(&source.prefix)
        && document[to..].starts_with(&source.suffix)
}

/// 截取不超过 48 个 UTF-16 单元的上下文，始终保留完整字符。
fn nearby_context(text: &str, before: bool) -> String {
    let chars: Vec<char> = if before {
        text.chars().rev().collect()
    } else {
        text.chars().collect()
    };
    let mut chosen = Vec::new();
    let mut length = 0;
    for c in chars {
        length += c.len_utf16();
        if length > 48 {
            break;
        }
        chosen.push(c);
    }
    if before {
        chosen.reverse();
    }
    chosen.into_iter().collect()
}

/// 仅在指定材料范围内唯一命中时建立坐标，重复摘录不猜测位置。
pub(crate) fn resolve_source(
    document: &str,
    excerpt: &str,
    scope: Option<&CardSource>,
) -> Option<CardSource> {
    if excerpt.trim().is_empty() {
        return None;
    }
    let (start, end) = if let Some(scope) = scope {
        if !validate_source(document, scope) {
            return None;
        }
        (
            byte_offset(document, scope.from)?,
            byte_offset(document, scope.to)?,
        )
    } else {
        (0, document.len())
    };
    let material = &document[start..end];
    let mut matches = material
        .char_indices()
        .filter_map(|(offset, _)| material[offset..].starts_with(excerpt).then_some(offset));
    let found = matches.next()?;
    if matches.next().is_some() {
        return None;
    }
    let from = start + found;
    let to = from + excerpt.len();
    Some(CardSource {
        from: document[..from].encode_utf16().count(),
        to: document[..to].encode_utf16().count(),
        excerpt: excerpt.to_string(),
        prefix: nearby_context(&document[..from], true),
        suffix: nearby_context(&document[to..], false),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    /// 判重字段按类型区分，格式变体不会绕过判重。
    fn identity_uses_kind_specific_field() {
        assert_eq!(
            card_identity("vocabulary", "甲", " ＡＢＣ  def "),
            card_identity("vocabulary", "乙", "abc def")
        );
        assert_eq!(
            card_identity("qa", "What  is RUST?", "甲"),
            card_identity("qa", "what is rust?", "乙")
        );
    }

    #[test]
    /// 重复或重叠摘录没有唯一定位；选区能够限定重复出现的位置。
    fn source_requires_unique_match_in_scope() {
        assert!(resolve_source("aaa", "aa", None).is_none());
        let scope = CardSource {
            from: 3,
            to: 5,
            excerpt: "甲乙".into(),
            prefix: "甲乙 ".into(),
            suffix: String::new(),
        };
        assert!(resolve_source("甲乙 甲乙", "甲乙", None).is_none());
        assert_eq!(
            resolve_source("甲乙 甲乙", "甲乙", Some(&scope))
                .unwrap()
                .from,
            3
        );
    }

    #[test]
    /// 前后文中的非 BMP 字符不会造成 Rust 与编辑器坐标错位。
    fn utf16_source_roundtrip_and_hash() {
        let source = resolve_source("🙂a知识点🙂", "知识点", None).unwrap();
        assert_eq!((source.from, source.to), (3, 6));
        assert!(validate_source("🙂a知识点🙂", &source));
        assert!(!validate_source("🙂b知识点🙂", &source));
        assert_eq!(note_content_hash("a\r\nb"), note_content_hash("a\nb"));
        assert_eq!(
            note_content_hash("abc"),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
    }

    #[test]
    /// 上下文窗口靠近 emoji 时不能产生无效代理项，也不能超过编辑器长度约定。
    fn context_respects_utf16_boundaries() {
        let document = format!("🙂{}目标{}🙂", "a".repeat(47), "b".repeat(47));
        let source = resolve_source(&document, "目标", None).unwrap();
        assert_eq!(source.prefix, "a".repeat(47));
        assert_eq!(source.suffix, "b".repeat(47));
        assert!(validate_source(&document, &source));
    }
}
