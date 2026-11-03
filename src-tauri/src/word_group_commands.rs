//! 单词组相关命令。
//!
//! 保存复用既有的 write_note（按扩展名委派 services::word_group::save），
//! 这里只提供"新建单词组文件"入口，命令层只做参数接收与委派。

use tauri::State;

use crate::{
    error::CommandError, models::NoteFile, services::word_group, storage::Storage,
    vaultfs::VaultState,
};

/// 在指定文件夹新建单词组文件（1 个空页），返回可打开的文件内容。
#[tauri::command]
pub async fn create_word_group(
    vault: State<'_, VaultState>,
    storage: State<'_, Storage>,
    folder: String,
    title: String,
) -> Result<NoteFile, CommandError> {
    word_group::create(vault.inner(), storage.inner(), &folder, &title).await
}
