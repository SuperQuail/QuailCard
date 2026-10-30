//! 卡片软分离：笔记进系统回收站时把镜像卡片文件挪进隔离区，笔记还原后再接回。
//!
//! 隔离区布局为 .quailcard/.detached/<与 active 卡片镜像相同的相对路径>，
//! 因此不新增任何 JSON 字段：接回时优先读文件内的 notePath，缺失则按
//! 相对路径推导。隔离文件不属于 active 数据：加载时跳过、损坏时跳过，
//! 只有超过保留期的孤儿才在打开 Vault 时清理。

use std::{
    path::{Path, PathBuf},
    time::SystemTime,
};

use super::{
    card_files::{card_relative_path, cleanup_empty_parents, derive_note_path, CARDS_DIR_NAME},
    card_records::NoteCardsFile,
    cards::CardState,
    envelope::{self, CorruptPolicy},
    now_timestamp,
};
use crate::error::CommandError;

/// 隔离区目录名，位于 .quailcard 之下；内部相对路径与 active 卡片镜像同构。
pub(super) const DETACHED_DIR_NAME: &str = ".detached";

/// 隔离卡片保留期（30 天）：覆盖用户从系统回收站还原笔记的合理窗口，
/// 超过后视为孤儿并清理，避免隔离区无限增长。
pub(super) const DETACHED_RETENTION_SECONDS: i64 = 30 * 24 * 60 * 60;

/// 隔离文件损坏或版本过高时跳过：隔离区不是 active 数据，
/// 不能因为一份旧备份无法解析而拒绝打开整个 Vault。
const DETACHED_CORRUPT: CorruptPolicy = CorruptPolicy::Reject {
    code: "DETACHED_FILE_INVALID",
    message: "隔离卡片文件不可读",
};

/// 判断相对 .quailcard 的路径是否位于隔离区；卡片加载必须跳过这些文件。
pub(super) fn is_detached_relative(relative: &str) -> bool {
    relative == DETACHED_DIR_NAME
        || relative
            .strip_prefix(DETACHED_DIR_NAME)
            .is_some_and(|rest| rest.starts_with('/'))
}

/// 把一篇笔记的卡片移出内存索引，并把 active 镜像文件挪进隔离区。
///
/// active 文件缺失视为成功：笔记文件可能已被外部删除，分离必须可重复
/// 执行。先把文件移走再改内存，移动失败时内存与磁盘仍保持一致。
pub(super) fn detach_note(
    state: &mut CardState,
    note_path: &str,
    root: &Path,
) -> Result<(), CommandError> {
    let source = root.join(card_relative_path(note_path)?);
    if source.is_file() {
        let target = detached_path(root, note_path)?;
        if let Some(parent) = target.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::rename(&source, &target)?;
        touch_detached(&target);
        // 传入文件路径：清理函数从其父目录开始向上删空目录。
        cleanup_empty_parents(root, Some(source.as_path()));
    }
    if let Some(cards) = state.notes.remove(note_path) {
        for card in &cards {
            state.card_note.remove(&card.id);
            state.idempotency.retain(|_, id| id != &card.id);
        }
    }
    Ok(())
}

/// 把隔离文件的 mtime 刷新为当前时间，作为保留期清理的起算点。
///
/// 尽力而为：时间戳只影响清理时机，刷新失败不能让删除笔记整体失败。
fn touch_detached(path: &Path) {
    let touched = std::fs::File::options()
        .write(true)
        .open(path)
        .and_then(|file| file.set_modified(SystemTime::now()));
    if let Err(error) = touched {
        eprintln!("DETACHED_TOUCH_ERROR(detail): {error} path={path:?}");
    }
}

/// 把笔记已回到 Vault 的隔离卡片文件移回 active 路径。
///
/// 必须在 load_card_files 之前执行，接回的文件才能在同一轮打开中被加载；
/// 笔记仍在回收站（或已被改名）时保留隔离文件，绝不接回没有源笔记的卡片。
/// 单文件失败只记日志并保留隔离副本，不能让一份旧备份阻止 Vault 打开。
pub(super) fn reattach_detached(root: &Path) {
    let detached_root = detached_root(root);
    let files = match walk_files(&detached_root) {
        Ok(files) => files,
        Err(error) => {
            eprintln!("DETACHED_REATTACH_SCAN_ERROR(detail): {error} path={detached_root:?}");
            return;
        }
    };
    for (relative, path) in files {
        // 只接回镜像卡片文件；其它残留（临时文件等）交给清理阶段。
        if !relative.to_lowercase().ends_with(".json") {
            continue;
        }
        let Some(file) = load_detached(&path) else {
            continue;
        };
        let note_path = if file.note_path.trim().is_empty() {
            derive_note_path(&relative)
        } else {
            file.note_path.trim().to_string()
        };
        // 文件内路径不可信：先净化再拼绝对路径，避免目录穿越。
        let Ok(sanitized) = crate::vaultfs::sanitize_relative(&note_path) else {
            eprintln!("DETACHED_REATTACH_SKIP(detail): note_path={note_path:?} path={path:?}");
            continue;
        };
        let note_path = sanitized.to_string_lossy().replace('\\', "/");
        if !root.join(&note_path).is_file() {
            continue;
        }
        if let Err(error) = restore_file(root, &note_path, &path) {
            eprintln!("DETACHED_REATTACH_ERROR(detail): {error} path={path:?}");
            continue;
        }
        cleanup_empty_parents(root, Some(path.as_path()));
    }
}

/// 读取隔离卡片文件；损坏、版本过高或读取失败时返回 None 并原样保留。
fn load_detached(path: &Path) -> Option<NoteCardsFile> {
    match envelope::load_json::<NoteCardsFile>(path, &DETACHED_CORRUPT) {
        Ok(file) => file,
        Err(error) => {
            eprintln!("DETACHED_FILE_SKIP(detail): {error} path={path:?}");
            None
        }
    }
}

/// 把单个隔离文件 rename 回 active 路径，父目录不存在时递归创建。
///
/// active 文件已存在时不覆盖：那可能是用户重新生成的卡片，保留隔离副本
/// 交给保留期清理，避免以旧覆盖新。
fn restore_file(root: &Path, note_path: &str, detached: &Path) -> Result<(), CommandError> {
    let active = root.join(card_relative_path(note_path)?);
    if active.exists() {
        eprintln!("DETACHED_REATTACH_CONFLICT(detail): path={active:?}");
        return Ok(());
    }
    if let Some(parent) = active.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::rename(detached, &active)?;
    Ok(())
}

/// 删除隔离区中超过保留期的文件，并清理随之变空的目录。
///
/// 尽力而为：清理只影响已过还原窗口的备份，失败只记日志，不能阻止打开 Vault。
pub(super) fn purge_detached(root: &Path) {
    let detached_root = detached_root(root);
    let files = match walk_files(&detached_root) {
        Ok(files) => files,
        Err(error) => {
            eprintln!("DETACHED_PURGE_SCAN_ERROR(detail): {error} path={detached_root:?}");
            return;
        }
    };
    let now = now_timestamp();
    for (_, path) in files {
        let Ok(modified) = std::fs::metadata(&path).and_then(|metadata| metadata.modified()) else {
            // 读不到修改时间就无法判断年龄，宁可保留也不误删用户数据。
            eprintln!("DETACHED_PURGE_SKIP(detail): path={path:?}");
            continue;
        };
        let modified = modified
            .duration_since(SystemTime::UNIX_EPOCH)
            .map(|duration| duration.as_secs() as i64)
            .unwrap_or_default();
        if now - modified <= DETACHED_RETENTION_SECONDS {
            continue;
        }
        if let Err(error) = std::fs::remove_file(&path) {
            eprintln!("DETACHED_PURGE_ERROR(detail): {error} path={path:?}");
            continue;
        }
        cleanup_empty_parents(root, Some(path.as_path()));
    }
}

/// 计算笔记对应隔离文件的绝对路径：与 active 镜像文件同构，只多一层目录。
fn detached_path(root: &Path, note_path: &str) -> Result<PathBuf, CommandError> {
    let relative = card_relative_path(note_path)?;
    let inner = relative
        .strip_prefix(CARDS_DIR_NAME)
        .map_err(|_| CommandError::new("INTERNAL_ERROR", "卡片相对路径缺少数据目录前缀"))?;
    Ok(detached_root(root).join(inner))
}

/// 返回隔离区根目录的绝对路径（目录可能尚不存在）。
fn detached_root(root: &Path) -> PathBuf {
    root.join(CARDS_DIR_NAME).join(DETACHED_DIR_NAME)
}

/// 递归收集隔离区内的普通文件，返回相对隔离区的 `/` 分隔路径与绝对路径。
fn walk_files(root: &Path) -> Result<Vec<(String, PathBuf)>, CommandError> {
    let mut files = Vec::new();
    if !root.is_dir() {
        return Ok(files);
    }
    let mut stack = vec![root.to_path_buf()];
    while let Some(directory) = stack.pop() {
        for entry in std::fs::read_dir(&directory)? {
            let entry = entry?;
            let path = entry.path();
            if path.is_dir() {
                stack.push(path);
                continue;
            }
            let relative = path
                .strip_prefix(root)
                .map(|relative| relative.to_string_lossy().replace('\\', "/"))
                .unwrap_or_default();
            files.push((relative, path));
        }
    }
    Ok(files)
}

#[cfg(test)]
#[path = "card_detach_tests.rs"]
mod detach_tests;
