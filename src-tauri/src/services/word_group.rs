//! 单词组用例：新建文件与保存编排。
//!
//! 保存顺序固定为：parse/校验 → 写文件 → upsert 索引 → 同步卡片。
//! 校验失败绝不落盘（保留用户草稿以便修正）；卡片同步失败时文件已保存，
//! 返回可重试的 WORD_GROUP_SYNC_ERROR，前端提示"已保存但卡片同步失败"。

use crate::{
    error::CommandError,
    models::NoteFile,
    storage::Storage,
    vaultfs::{documents::DocumentKind, VaultState},
    word_group,
};

/// 新建单词组：生成 1 个空页文档并写入文件，返回可打开的文件内容。
///
/// 初始页是草稿不会产生卡片，但仍同步一次：若该路径残留了旧卡片
/// （例如文件被外部删除后重建），可在此清理，避免显示过期卡片。
pub(crate) async fn create(
    vault: &VaultState,
    storage: &Storage,
    folder: &str,
    title: &str,
) -> Result<NoteFile, CommandError> {
    let document = word_group::initial_document();
    let content = word_group::serialize(&document)?;
    let path = vault.create_word_group_file(folder, title, &content)?;
    let (content, mtime) = vault.read_note(&path)?;
    storage.upsert_note_index(&path, &content, mtime).await?;
    // 空文档同步只会删除残留卡片；失败不影响新建结果，记日志即可。
    if let Err(error) = storage.sync_word_group_cards(&path, &document) {
        eprintln!("WORD_GROUP_CREATE_SYNC_ERROR(detail): {error} path={path}");
    }
    Ok(NoteFile {
        title: crate::vaultfs::documents::title_from_path(&path),
        path,
        content,
        mtime,
    })
}

/// 保存单词组：先校验再落盘，随后同步索引与卡片，返回新修改时间。
pub(crate) async fn save(
    vault: &VaultState,
    storage: &Storage,
    path: &str,
    content: &str,
) -> Result<i64, CommandError> {
    if DocumentKind::of_path(path) != Some(DocumentKind::WordGroup) {
        return Err(CommandError::validation("不是单词组文件"));
    }
    // 非法 JSON、版本过高与结构校验失败都在写盘前拒绝。
    let document = word_group::parse(content)?;
    let mtime = vault.write_note(path, content)?;
    storage.upsert_note_index(path, content, mtime).await?;
    if let Err(error) = storage.sync_word_group_cards(path, &document) {
        eprintln!("WORD_GROUP_SYNC_ERROR(detail): {error} path={path}");
        return Err(CommandError::new(
            "WORD_GROUP_SYNC_ERROR",
            "单词组已保存，但卡片同步失败，请重试",
        ));
    }
    Ok(mtime)
}

#[cfg(test)]
#[path = "word_group_tests.rs"]
mod tests;
