//! .quailcard 镜像卡片文件的磁盘布局与 IO 辅助。
//!
//! 职责边界：本模块只负责卡片文件在磁盘上的定位、加载、原子写透与
//! 目录清理；内存缓存结构（CardStore/CardState）见 cards 模块。

use std::{
    path::{Path, PathBuf},
    sync::RwLock,
};

use super::{
    card_records::{CardRecord, NoteCardsFile},
    cards::CardState,
    envelope::{self, CorruptPolicy},
};
use crate::error::CommandError;

/// Vault 内保留的数据目录名，与附件配置共用。
pub(crate) const CARDS_DIR_NAME: &str = ".quailcard";

/// 卡片文件损坏时直接拒开，绝不自动重置（不可再生数据）。
const CARDS_CORRUPT: CorruptPolicy = CorruptPolicy::Reject {
    code: "CARD_FILE_CORRUPT",
    message: "卡片文件已损坏，为防止数据丢失已停止加载，请手动处理后重试",
};

/// 递归加载 .quailcard 下全部卡片文件到内存状态。
pub(super) fn load_card_files(root: &Path, state: &mut CardState) -> Result<(), CommandError> {
    let cards_dir = root.join(CARDS_DIR_NAME);
    if !cards_dir.is_dir() {
        return Ok(());
    }
    let mut stack = vec![cards_dir.clone()];
    while let Some(directory) = stack.pop() {
        for entry in std::fs::read_dir(&directory)? {
            let entry = entry?;
            let path = entry.path();
            if path.is_dir() {
                stack.push(path);
                continue;
            }
            let relative = match path.strip_prefix(&cards_dir) {
                Ok(relative) => relative.to_string_lossy().replace('\\', "/"),
                Err(_) => continue,
            };
            // 顶层 config.json 是附件目录配置，不属于卡片数据。
            if relative == "config.json" || !relative.to_lowercase().ends_with(".json") {
                continue;
            }
            let Some(file) = envelope::load_json::<NoteCardsFile>(&path, &CARDS_CORRUPT)? else {
                continue;
            };
            let note_path = if file.note_path.trim().is_empty() {
                derive_note_path(&relative)
            } else {
                file.note_path
            };
            for card in file.cards {
                state.card_note.insert(card.id.clone(), note_path.clone());
                for record in &card.history {
                    state
                        .idempotency
                        .insert(record.idempotency_key.clone(), card.id.clone());
                }
                state.notes.entry(note_path.clone()).or_default().push(card);
            }
        }
    }
    for cards in state.notes.values_mut() {
        cards.sort_by_key(|card| card.position);
    }
    Ok(())
}

/// 从镜像文件相对路径推导笔记路径（词.json -> 词.md）。
fn derive_note_path(relative: &str) -> String {
    let trimmed = relative.trim_end_matches(".json");
    format!("{trimmed}.md")
}

/// 计算笔记路径对应的镜像卡片文件相对路径。
pub(super) fn card_relative_path(note_path: &str) -> Result<PathBuf, CommandError> {
    let sanitized = crate::vaultfs::sanitize_relative(note_path)?;
    let mut name = sanitized.to_string_lossy().to_string();
    if name.to_lowercase().ends_with(".md") {
        name.truncate(name.len() - 3);
    }
    name.push_str(".json");
    Ok(Path::new(CARDS_DIR_NAME).join(name))
}

/// 把单篇笔记的卡片列表原子写透磁盘；清空时移除文件。
pub(super) fn persist_cards(
    root: &Path,
    note_path: &str,
    cards: &[CardRecord],
) -> Result<(), CommandError> {
    let path = root.join(card_relative_path(note_path)?);
    if cards.is_empty() {
        if path.exists() {
            std::fs::remove_file(&path)?;
            cleanup_empty_parents(root, path.parent());
        }
        return Ok(());
    }
    envelope::save_json(
        &path,
        &NoteCardsFile {
            format_version: envelope::CURRENT_FORMAT_VERSION,
            note_path: note_path.to_string(),
            cards: cards.to_vec(),
        },
    )
}

/// 从笔记卡片列表移除单张卡片并同步磁盘与索引。
pub(super) fn remove_card_from_note(
    state: &mut CardState,
    note_path: &str,
    card_id: &str,
    root: &Path,
) -> Result<(), CommandError> {
    let Some(cards) = state.notes.get_mut(note_path) else {
        return Err(CommandError::new("CARD_NOT_FOUND", "卡片不存在"));
    };
    let before = cards.len();
    cards.retain(|card| card.id != card_id);
    if cards.len() == before {
        return Err(CommandError::new("CARD_NOT_FOUND", "卡片不存在"));
    }
    let remaining = if cards.is_empty() {
        state.notes.remove(note_path);
        Vec::new()
    } else {
        cards.clone()
    };
    persist_cards(root, note_path, &remaining)?;
    Ok(())
}

/// 移除一篇笔记的全部卡片、镜像文件与索引项。
pub(super) fn remove_note_cards(
    state: &mut CardState,
    note_path: &str,
    root: &Path,
) -> Result<(), CommandError> {
    let Some(cards) = state.notes.remove(note_path) else {
        return Ok(());
    };
    for card in &cards {
        state.card_note.remove(&card.id);
        state.idempotency.retain(|_, id| id != &card.id);
    }
    let path = root.join(card_relative_path(note_path)?);
    if path.exists() {
        std::fs::remove_file(&path)?;
        cleanup_empty_parents(root, path.parent());
    }
    Ok(())
}

/// 尽力清理移动/删除后留下的空镜像目录，直到 .quailcard 根为止。
pub(super) fn cleanup_empty_parents(root: &Path, start: Option<&Path>) {
    let mut current = match start.and_then(Path::parent) {
        Some(parent) => parent.to_path_buf(),
        None => return,
    };
    let boundary = root.join(CARDS_DIR_NAME);
    while current.starts_with(&boundary) && current != boundary {
        if std::fs::remove_dir(&current).is_err() {
            break;
        }
        if !current.pop() {
            break;
        }
    }
}

/// 要求 Vault 已打开，否则返回统一错误。
pub(super) fn require_root(state: &CardState) -> Result<PathBuf, CommandError> {
    state
        .root
        .clone()
        .ok_or_else(|| CommandError::new("VAULT_NOT_OPEN", "请先选择 Vault 文件夹"))
}

/// 获取写锁；中毒时视为内部错误。
pub(super) fn lock_write<T>(
    lock: &RwLock<T>,
) -> Result<std::sync::RwLockWriteGuard<'_, T>, CommandError> {
    lock.write()
        .map_err(|_| CommandError::new("INTERNAL_ERROR", "卡片存储状态锁失效"))
}
