//! 反链查询：找出正文里引用了指定笔记的其他笔记。
//!
//! 关系在查询时对内存索引快照现算：链接表的规模与笔记数同阶，线性扫描
//! 与现有全文检索同一量级；不维护反向索引，避免改名/删除时还要同步第二份状态。

use super::{
    links::{self, ResolveNote},
    Storage,
};
use crate::{error::CommandError, models::Backlink};

/// 反链预览的最大字符数：够看清上下文，又不至于把侧栏撑开。
const MAX_SNIPPET_CHARS: usize = 160;

impl Storage {
    /// 查询指向 path 的全部反链，按来源标题与行号排序并去重。
    ///
    /// 契约：只统计能解析到目标笔记的双链；解析规则与编辑器完全一致
    /// （同一份 links::resolve）。同一行出现两处链接只保留一条预览。
    pub async fn backlinks(&self, path: &str) -> Result<Vec<Backlink>, CommandError> {
        let entries = self.inner.notes.snapshot();
        // ResolveNote 借用索引条目，借用期内不能再移动 entries。
        let notes: Vec<ResolveNote<'_>> = entries
            .iter()
            .map(|entry| ResolveNote {
                path: &entry.path,
                title: &entry.title,
                aliases: &entry.aliases,
            })
            .collect();
        let mut found: Vec<Backlink> = Vec::new();
        for entry in &entries {
            if entry.path == path {
                continue;
            }
            for link in &entry.links {
                if links::resolve(&link.target, &entry.path, &notes).as_deref() != Some(path) {
                    continue;
                }
                let Some(line_number) = line_number_at(&entry.content, link.from) else {
                    continue;
                };
                let snippet = line_text(&entry.content, link.from);
                if found
                    .iter()
                    .any(|item| item.source_path == entry.path && item.line == line_number)
                {
                    continue;
                }
                found.push(Backlink {
                    source_path: entry.path.clone(),
                    source_title: entry.title.clone(),
                    line: line_number,
                    snippet,
                });
            }
        }
        // 与笔记列表同一套排序直觉：标题小写后比较，再用行号与路径保证稳定。
        found.sort_by(|left, right| {
            left.source_title
                .to_lowercase()
                .cmp(&right.source_title.to_lowercase())
                .then_with(|| left.line.cmp(&right.line))
                .then_with(|| left.source_path.cmp(&right.source_path))
        });
        Ok(found)
    }
}

/// 把 UTF-16 偏移换算成 1 起始的行号；偏移越界返回 None。
fn line_number_at(content: &str, offset: usize) -> Option<usize> {
    let byte = links::utf16_to_byte(content, offset)?;
    Some(content[..byte].matches('\n').count() + 1)
}

/// 取偏移所在行去首尾空白后的预览文本，超长时截断并加省略号。
fn line_text(content: &str, offset: usize) -> String {
    let Some(byte) = links::utf16_to_byte(content, offset) else {
        return String::new();
    };
    let start = content[..byte].rfind('\n').map_or(0, |index| index + 1);
    let end = content[byte..]
        .find('\n')
        .map_or(content.len(), |index| byte + index);
    let line = content[start..end].trim();
    if line.chars().count() <= MAX_SNIPPET_CHARS {
        return line.to_string();
    }
    let mut snippet: String = line.chars().take(MAX_SNIPPET_CHARS).collect();
    snippet.push('…');
    snippet
}
