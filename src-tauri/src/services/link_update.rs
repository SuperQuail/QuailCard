//! 改名联动：重命名笔记或文件夹后，自动改写其他笔记里指向它的双链。
//!
//! 为什么必须先规划再执行：改名之后索引里就没有旧路径了，\`[[旧名]]\` 到底
//! 指向谁将无从判断。因此命令层在改名**前**调用 [plan] 记录"哪些链接指向
//! 被移动的笔记"，改名后再用 [apply] 按新索引决定写法并落盘。

use std::collections::BTreeMap;

use crate::{
    error::CommandError,
    models::RenameOutcome,
    storage::{
        helpers::note_title_from_path,
        links::{self, RawLink, ResolveNote},
        Storage,
    },
    vaultfs::VaultState,
};

/// 一条待改写的链接：所在笔记、链接快照与改写前解析到的目标。
pub(crate) struct PendingRewrite {
    /// 链接所在的笔记路径（改写前的路径）。
    pub(crate) note_path: String,
    /// 链接本身（含 UTF-16 坐标）。
    pub(crate) link: RawLink,
    /// 改写前该链接解析到的笔记路径。
    pub(crate) resolved: String,
}

/// 改名前收集所有指向被移动笔记的链接。
///
/// 契约：只收集能解析到 [old_prefix] 及其子路径（文件夹改名）的链接；
/// 解析规则与编辑器、反链完全一致，避免"界面显示有链接但改名不跟着改"。
pub(crate) async fn plan(storage: &Storage, old_prefix: &str) -> Vec<PendingRewrite> {
    let snapshot = storage.link_snapshot().await;
    let notes: Vec<ResolveNote<'_>> = snapshot
        .iter()
        .map(|entry| ResolveNote {
            path: &entry.path,
            title: &entry.title,
            aliases: &entry.aliases,
        })
        .collect();
    let mut pending = Vec::new();
    for entry in &snapshot {
        for link in &entry.links {
            let Some(resolved) = links::resolve(&link.target, &entry.path, &notes) else {
                continue;
            };
            if !is_moved(&resolved, old_prefix) {
                continue;
            }
            pending.push(PendingRewrite {
                note_path: entry.path.clone(),
                link: link.clone(),
                resolved,
            });
        }
    }
    pending
}

/// 判断路径是否落在被移动的前缀里（单篇是精确匹配，文件夹是前缀匹配）。
fn is_moved(path: &str, old_prefix: &str) -> bool {
    path == old_prefix || path.starts_with(&format!("{old_prefix}/"))
}

/// 把被移动笔记的旧路径换算成新路径。
fn moved_path(path: &str, old_prefix: &str, new_prefix: &str) -> String {
    if path == old_prefix {
        new_prefix.to_string()
    } else {
        format!("{new_prefix}/{}", &path[old_prefix.len() + 1..])
    }
}

/// 决定改写后的链接写法：原来用裸标题且新标题仍然唯一时继续用裸标题，否则写相对路径。
///
/// 保持原写法是为了让 diff 尽量小、和用户手写的风格一致；一旦有重名就必须
/// 退化成路径形式，否则改名会把链接指向另一篇同名笔记。
fn replacement_text(new_path: &str, original_target: &str, notes: &[ResolveNote<'_>]) -> String {
    let new_title = note_title_from_path(new_path);
    let used_path_form = original_target.contains('/') || original_target.contains('\\');
    if !used_path_form && title_is_unique(&new_title, notes) {
        return new_title;
    }
    strip_md_suffix(new_path)
}

/// 判断标题在改名后的索引里是否唯一（忽略大小写）。
///
/// 只统计标题相同的笔记数：等于 1 说明裸标题写法不会歧义，可以保持原写法。
fn title_is_unique(title: &str, notes: &[ResolveNote<'_>]) -> bool {
    let wanted = links::normalize_key(title);
    notes
        .iter()
        .filter(|note| links::normalize_key(note.title) == wanted)
        .count()
        <= 1
}

/// 去掉 .md 后缀：双链里的路径写法不写扩展名。
fn strip_md_suffix(path: &str) -> String {
    path.strip_suffix(".md")
        .or_else(|| path.strip_suffix(".MD"))
        .unwrap_or(path)
        .to_string()
}

/// 执行改写：按笔记分组，逐篇原子写入并同步索引与卡片锚点。
///
/// 失败隔离：单篇写入失败只记入 failed_notes 并继续处理下一篇；
/// 文件改名本身不回滚，与既有"先改文件再改存储"的非原子语义保持一致。
pub(crate) async fn apply(
    vault: &VaultState,
    storage: &Storage,
    pending: Vec<PendingRewrite>,
    old_prefix: &str,
    new_prefix: &str,
) -> Result<RewriteReport, CommandError> {
    let mut grouped: BTreeMap<String, Vec<PendingRewrite>> = BTreeMap::new();
    for item in pending {
        grouped
            .entry(item.note_path.clone())
            .or_default()
            .push(item);
    }
    if grouped.is_empty() {
        return Ok(RewriteReport::default());
    }
    let snapshot = storage.link_snapshot().await;
    let notes: Vec<ResolveNote<'_>> = snapshot
        .iter()
        .map(|entry| ResolveNote {
            path: &entry.path,
            title: &entry.title,
            aliases: &entry.aliases,
        })
        .collect();
    let mut report = RewriteReport::default();
    for (note_path, items) in grouped {
        let Some(entry) = snapshot.iter().find(|entry| entry.path == note_path) else {
            report.failed.push(note_path);
            continue;
        };
        let matches: Vec<(RawLink, String)> = items
            .into_iter()
            .map(|item| {
                let next_path = moved_path(&item.resolved, old_prefix, new_prefix);
                let replacement = replacement_text(&next_path, &item.link.target, &notes);
                (item.link, replacement)
            })
            .collect();
        let (content, edits) = links::rewrite_links(&entry.content, &matches);
        if edits.is_empty() {
            continue;
        }
        match vault.write_note(&note_path, &content) {
            Ok(mtime) => {
                storage
                    .upsert_note_index(&note_path, &content, mtime)
                    .await?;
                if let Err(error) = storage.shift_card_sources(&note_path, &edits).await {
                    // 卡片是派生数据；锚点没跟上只影响"来源已失效"提示，不影响笔记本身。
                    eprintln!("平移卡片锚点失败 {note_path}: {}", error.message);
                }
                report.updated.push(note_path);
            }
            Err(error) => {
                eprintln!("改写链接失败 {note_path}: {}", error.message);
                report.failed.push(note_path);
            }
        }
    }
    Ok(report)
}

/// 改写结果：成功与失败的笔记路径，直接映射为 [RenameOutcome]。
#[derive(Default)]
pub(crate) struct RewriteReport {
    pub(crate) updated: Vec<String>,
    pub(crate) failed: Vec<String>,
}

impl RewriteReport {
    /// 组装改名结果；文件名改写本身总是成功的，失败只体现在链接改写上。
    pub(crate) fn into_outcome(self, path: String) -> RenameOutcome {
        RenameOutcome {
            path,
            updated_notes: self.updated,
            failed_notes: self.failed,
        }
    }
}
