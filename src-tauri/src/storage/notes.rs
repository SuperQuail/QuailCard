//! 笔记内存索引：由 Vault 扫描重建，不再持久化。
//!
//! 原方案把笔记正文缓存在 SQLite（note_index + FTS）；文件方案下
//! 磁盘上的 .md 文件本身就是事实来源，内存索引只为列表与搜索加速。

use std::sync::RwLock;

use super::{frontmatter, helpers, links, notes_summary_sort_key, Storage};
use crate::{
    error::CommandError, models::NoteSummary, vaultfs::documents::DocumentKind, word_group,
};

/// 汇总一篇笔记的标签与别名。
///
/// 契约：frontmatter 声明的标签排在正文行内标签之前（作者显式声明更权威），
/// 合并去重后仍受行内标签的数量上限约束；别名只来自 frontmatter。
fn collect_tags(content: &str) -> (Vec<String>, Vec<String>) {
    let (meta, body) = frontmatter::parse_content(content);
    let mut tags = meta.tags;
    for tag in helpers::extract_tags(body) {
        if !tags.contains(&tag) {
            tags.push(tag);
        }
        if tags.len() >= 20 {
            break;
        }
    }
    tags.truncate(20);
    (tags, meta.aliases)
}

/// 构造索引条目：Markdown 解析 frontmatter 与双链，单词组只派生搜索文本。
///
/// 契约：单词组没有 frontmatter，也不参与双链，标签/别名/链接一律为空；
/// content 置空以免被误当正文；损坏的单词组不影响列表，只是搜索不到内容。
fn build_entry(path: &str, content: &str, mtime: i64) -> NoteEntry {
    let kind = DocumentKind::of_path(path).unwrap_or(DocumentKind::Markdown);
    let mut entry = NoteEntry {
        path: path.to_string(),
        title: helpers::note_title_from_path(path),
        kind,
        tags: Vec::new(),
        aliases: Vec::new(),
        links: Vec::new(),
        mtime,
        content: String::new(),
        search_text: None,
    };
    if kind.is_markdown() {
        let (tags, aliases) = collect_tags(content);
        entry.tags = tags;
        entry.aliases = aliases;
        entry.links = links::scan_content_links(content);
        entry.content = content.to_string();
        return entry;
    }
    let search_text = match word_group::parse(content) {
        Ok(document) => word_group::search_text(&document),
        Err(error) => {
            // 损坏的单词组仍然列出（编辑器只读提示），只是搜索不到内容。
            eprintln!("WORD_GROUP_INDEX_SKIP(detail): {error} path={path}");
            String::new()
        }
    };
    entry.search_text = Some(search_text);
    entry
}

/// 笔记内存索引；条目按（标题, 路径）稳定排序。
#[derive(Default)]
pub(crate) struct NoteIndex {
    entries: RwLock<Vec<NoteEntry>>,
}

/// 单篇文档的索引条目；content 与 search_text 仅供内存搜索使用。
#[derive(Debug, Clone)]
pub(crate) struct NoteEntry {
    pub(crate) path: String,
    pub(crate) title: String,
    /// 文档类型：单词组不解析标签、别名与双链。
    pub(crate) kind: DocumentKind,
    pub(crate) tags: Vec<String>,
    /// frontmatter 声明的别名：双链解析时与标题同等对待。
    pub(crate) aliases: Vec<String>,
    /// 正文中指向其他笔记的双链（文档 UTF-16 坐标），反链查询与改名改写都用它。
    pub(crate) links: Vec<links::RawLink>,
    pub(crate) mtime: i64,
    /// Markdown 正文；单词组置空，避免被当作 Markdown 渲染或解析。
    pub(crate) content: String,
    /// Markdown 为 None（搜索直接用 content）；单词组为派生搜索文本。
    pub(crate) search_text: Option<String>,
}

impl NoteIndex {
    /// 用一次完整扫描结果整体替换索引。
    pub(crate) fn rebuild(&self, files: &[(String, String, i64)]) {
        let mut entries: Vec<NoteEntry> = files
            .iter()
            .map(|(path, content, mtime)| build_entry(path, content, *mtime))
            .collect();
        entries.sort_by(|left, right| {
            notes_summary_sort_key(&left.title, &left.path)
                .cmp(&notes_summary_sort_key(&right.title, &right.path))
        });
        if let Ok(mut guard) = self.entries.write() {
            *guard = entries;
        }
    }

    /// 单篇笔记保存后新增或替换索引条目，保持排序不变量。
    pub(crate) fn upsert(&self, path: &str, content: &str, mtime: i64) {
        let entry = build_entry(path, content, mtime);
        if let Ok(mut guard) = self.entries.write() {
            guard.retain(|existing| existing.path != path);
            guard.push(entry);
            guard.sort_by(|left, right| {
                notes_summary_sort_key(&left.title, &left.path)
                    .cmp(&notes_summary_sort_key(&right.title, &right.path))
            });
        }
    }

    /// 克隆全部条目快照（列表与搜索的读取基础）。
    pub(super) fn snapshot(&self) -> Vec<NoteEntry> {
        self.entries
            .read()
            .map(|guard| guard.clone())
            .unwrap_or_default()
    }

    /// 重命名文件夹后批量替换路径前缀。
    pub(super) fn rename_prefix(&self, old_prefix: &str, new_prefix: &str) {
        if let Ok(mut guard) = self.entries.write() {
            for entry in guard.iter_mut() {
                if entry.path == old_prefix {
                    entry.path = new_prefix.to_string();
                } else if let Some(rest) = entry.path.strip_prefix(&format!("{old_prefix}/")) {
                    entry.path = format!("{new_prefix}/{rest}");
                }
                entry.title = helpers::note_title_from_path(&entry.path);
            }
            guard.sort_by(|left, right| {
                notes_summary_sort_key(&left.title, &left.path)
                    .cmp(&notes_summary_sort_key(&right.title, &right.path))
            });
        }
    }

    /// 移除单篇笔记条目。
    pub(super) fn remove(&self, path: &str) {
        if let Ok(mut guard) = self.entries.write() {
            guard.retain(|existing| existing.path != path);
        }
    }

    /// 移除文件夹前缀下全部条目。
    pub(super) fn remove_prefix(&self, prefix: &str) {
        let child_prefix = format!("{prefix}/");
        if let Ok(mut guard) = self.entries.write() {
            guard.retain(|existing| {
                existing.path != prefix && !existing.path.starts_with(&child_prefix)
            });
        }
    }
}

impl Storage {
    /// 打开 Vault：记录根目录、加载卡片文件、重建笔记索引并对账单词组卡片。
    pub async fn open_vault(
        &self,
        root: &std::path::Path,
        files: &[(String, String, i64)],
    ) -> Result<(), CommandError> {
        self.inner.cards.set_root(root)?;
        self.inner.notes.rebuild(files);
        // 单词组以文件为事实来源：打开/重扫时补齐卡片，无变化不写盘。
        self.reconcile_word_group_cards(files);
        Ok(())
    }

    /// 重新扫描 Vault：以磁盘为准整体刷新卡片缓存、笔记索引与单词组卡片。
    pub async fn rescan_vault(
        &self,
        root: &std::path::Path,
        files: &[(String, String, i64)],
    ) -> Result<(), CommandError> {
        self.inner.cards.set_root(root)?;
        self.inner.notes.rebuild(files);
        self.reconcile_word_group_cards(files);
        Ok(())
    }

    /// 新增或更新单篇笔记的索引缓存。
    pub async fn upsert_note_index(
        &self,
        path: &str,
        content: &str,
        mtime: i64,
    ) -> Result<(), CommandError> {
        self.inner.notes.upsert(path, content, mtime);
        Ok(())
    }

    /// 查询全部笔记摘要及实时卡片统计。
    pub async fn list_notes(&self) -> Result<Vec<NoteSummary>, CommandError> {
        let entries = self.inner.notes.snapshot();
        let counts = self.inner.cards.note_counts(super::now_timestamp());
        Ok(entries
            .iter()
            .map(|entry| {
                let (card_count, due_count) = counts.get(&entry.path).cloned().unwrap_or((0, 0));
                NoteSummary {
                    path: entry.path.clone(),
                    title: entry.title.clone(),
                    tags_json: serde_json::to_string(&entry.tags).unwrap_or_else(|_| "[]".into()),
                    aliases: entry.aliases.clone(),
                    card_count,
                    due_count,
                    mtime: entry.mtime,
                }
            })
            .collect())
    }

    /// 重命名笔记/文件夹后同步索引与卡片路径。
    pub async fn rename_note_paths(
        &self,
        old_prefix: &str,
        new_prefix: &str,
    ) -> Result<(), CommandError> {
        self.inner.cards.rename_note_paths(old_prefix, new_prefix)?;
        self.inner.notes.rename_prefix(old_prefix, new_prefix);
        Ok(())
    }

    /// 删除笔记文件后清理其索引与全部卡片。
    pub async fn delete_note_paths(&self, path: &str) -> Result<(), CommandError> {
        self.inner.notes.remove(path);
        self.inner.cards.delete_note_paths(path)
    }

    /// 删除文件夹后清理其下全部索引与卡片。
    pub async fn delete_folder_paths(&self, prefix: &str) -> Result<(), CommandError> {
        self.inner.notes.remove_prefix(prefix);
        self.inner.cards.delete_folder_paths(prefix)
    }

    /// 笔记移入回收站后，把索引与卡片从活跃集合中分离；卡片文件保留在
    /// 隔离区，用户从系统回收站还原笔记后还能一并接回。
    pub async fn detach_note_paths(&self, path: &str) -> Result<(), CommandError> {
        self.inner.notes.remove(path);
        self.inner.cards.detach_note_paths(path)
    }

    /// 文件夹移入回收站后，分离其下全部索引与卡片。
    pub async fn detach_folder_paths(&self, prefix: &str) -> Result<(), CommandError> {
        self.inner.notes.remove_prefix(prefix);
        self.inner.cards.detach_folder_paths(prefix)
    }
}

/// 双链联动与反链查询共用的一篇笔记快照。
pub(crate) struct LinkSnapshotEntry {
    pub(crate) path: String,
    pub(crate) title: String,
    /// 文档类型；快照当前只含 Markdown，字段保留给用例层做防御性过滤。
    pub(crate) kind: DocumentKind,
    pub(crate) aliases: Vec<String>,
    pub(crate) links: Vec<links::RawLink>,
    pub(crate) content: String,
}

impl Storage {
    /// 克隆全部 Markdown 笔记的双链相关快照：链接解析、反链与改名改写都基于它计算。
    ///
    /// 契约：单词组不参与双链，候选集合里绝不能出现 .qword，否则同名标题会
    /// 抢走 [[链接]] 的解析结果。快照与索引同源，调用方拿到的是拷贝。
    pub(crate) async fn link_snapshot(&self) -> Vec<LinkSnapshotEntry> {
        self.inner
            .notes
            .snapshot()
            .into_iter()
            .filter(|entry| entry.kind.is_markdown())
            .map(|entry| LinkSnapshotEntry {
                path: entry.path,
                title: entry.title,
                kind: entry.kind,
                aliases: entry.aliases,
                links: entry.links,
                content: entry.content,
            })
            .collect()
    }
}
