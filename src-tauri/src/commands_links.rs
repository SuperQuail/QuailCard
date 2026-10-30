//! 双链相关命令：反链查询。
//!
//! 放在独立文件而不是 commands.rs：后者已接近行数上限，
//! 且双链是横切能力，不属于"笔记文件读写"这一组。

use tauri::State;

use crate::{error::CommandError, models::Backlink, storage::Storage};

/// 查询指向某篇笔记的全部反链。
///
/// 规则与编辑器里的 [[双链]] 解析完全一致（同一份 links::resolve）：
/// 界面上能点开的链接，这里一定能查到。
#[tauri::command]
pub async fn get_backlinks(
    storage: State<'_, Storage>,
    path: String,
) -> Result<Vec<Backlink>, CommandError> {
    storage.backlinks(&path).await
}
