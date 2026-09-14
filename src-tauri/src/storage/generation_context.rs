//! 拆卡会话的磁盘事实校验：固定 Vault、笔记正文和选区。

use std::path::Path;

use super::{card_files::require_root, Storage};
use crate::{
    card_generation::{note_content_hash, validate_source},
    error::CommandError,
    models::GenerationInput,
    services::generation_ports::GenerationSnapshot,
    vaultfs::sanitize_relative,
};

impl Storage {
    /// 在同一个卡片状态快照中校验材料，防止换 Vault 后沿用旧笔记上下文。
    pub(crate) fn validate_generation_context(
        &self,
        input: &GenerationInput,
    ) -> Result<GenerationSnapshot, CommandError> {
        let Some(context) = &input.context else {
            return Ok(GenerationSnapshot {
                note_content: input.source_text.replace("\r\n", "\n"),
                cards: Vec::new(),
            });
        };
        let state = self
            .inner
            .cards
            .state
            .read()
            .map_err(|_| CommandError::new("INTERNAL_ERROR", "卡片存储状态锁失效"))?;
        let root = require_root(&state)?;
        let content = read_context_note(&root, &context.vault_path, &context.note_path)?;
        ensure_note_hash(&content, &context.note_hash)?;
        let material = input.source_text.replace("\r\n", "\n");
        let matches = context.selection.as_ref().map_or_else(
            || material == content,
            |source| validate_source(&content, source) && material == source.excerpt,
        );
        if !matches {
            return Err(CommandError::new(
                "GENERATION_SOURCE_CHANGED",
                "拆卡材料与已保存的笔记或选区不一致，请重新打开拆卡窗口",
            ));
        }
        let cards = state
            .notes
            .get(&context.note_path)
            .into_iter()
            .flatten()
            .map(|card| card.to_note_card(&context.note_path))
            .collect();
        Ok(GenerationSnapshot {
            note_content: content,
            cards,
        })
    }
}

/// 验证固定 Vault 并读取净化后的笔记；最终目标也须在 Vault 内，拒绝符号链接越界。
pub(super) fn read_context_note(
    root: &Path,
    expected_vault: &str,
    note_path: &str,
) -> Result<String, CommandError> {
    let root = root.canonicalize()?;
    let expected = Path::new(expected_vault).canonicalize().map_err(|_| {
        CommandError::new("GENERATION_VAULT_CHANGED", "原 Vault 已不可用，请重新生成")
    })?;
    if root != expected {
        return Err(CommandError::new(
            "GENERATION_VAULT_CHANGED",
            "Vault 已切换，请重新生成",
        ));
    }
    let relative = sanitize_relative(note_path)?;
    // 禁止同一笔记以 ./ 等别名建立第二份卡片索引。
    if relative
        .components()
        .any(|part| matches!(part, std::path::Component::CurDir))
        || relative
            .components()
            .collect::<std::path::PathBuf>()
            .to_string_lossy()
            .replace('\\', "/")
            != note_path
    {
        return Err(CommandError::validation(
            "笔记路径必须为规范的 Vault 相对路径",
        ));
    }
    let path = root
        .join(relative)
        .canonicalize()
        .map_err(|_| CommandError::new("NOTE_NOT_FOUND", "原笔记已删除或重命名，请重新生成"))?;
    if !path.starts_with(&root) {
        return Err(CommandError::validation("路径超出 Vault 范围"));
    }
    if !path.is_file() {
        return Err(CommandError::new(
            "NOTE_NOT_FOUND",
            "原笔记已删除或重命名，请重新生成",
        ));
    }
    let content = String::from_utf8(std::fs::read(path)?)
        .map_err(|_| CommandError::new("FILE_ENCODING", "笔记不是有效的 UTF-8 文本"))?;
    Ok(content.replace("\r\n", "\n"))
}

/// 正文摘要只用于检测生成后的材料变化，不作为授权或凭据。
pub(super) fn ensure_note_hash(content: &str, expected: &str) -> Result<(), CommandError> {
    if note_content_hash(content) != expected {
        return Err(CommandError::new(
            "GENERATION_NOTE_CHANGED",
            "笔记正文已改变，草稿已保留，请重新生成后采纳",
        ));
    }
    Ok(())
}
