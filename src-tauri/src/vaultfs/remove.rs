//! 删除策略：系统回收站与永久删除的统一抽象。
//!
//! 删除是唯一需要跟随用户设置切换行为的文件操作，因此把"怎么删"抽成
//! trait，命令层只负责选策略，不在业务代码里散布平台细节。将来若出现
//! 第三种删除方式（例如移到 Vault 内的隔离目录），新增一个实现即可。

use std::path::Path;

use super::VaultState;
use crate::error::CommandError;

/// 回收站不可用时的统一错误码；前端据此询问是否改为永久删除。
const TRASH_UNAVAILABLE: &str = "TRASH_UNAVAILABLE";

/// 删除策略：把文件或目录（含其全部内容）从 Vault 中移除。
///
/// 契约：成功时目标必须已从原位置消失；失败时目标保持原样，不出现
/// 删了一半的中间态。回收站由系统 API 保证，永久删除由文件系统保证。
pub(crate) trait Remover: Send + Sync {
    /// 移除单个文件。
    fn remove_file(&self, path: &Path) -> Result<(), CommandError>;

    /// 递归移除目录及其全部内容。
    fn remove_dir_all(&self, path: &Path) -> Result<(), CommandError>;
}

/// 永久删除策略：直接走文件系统删除，不可恢复。
pub(crate) struct PermanentRemover;

impl Remover for PermanentRemover {
    /// 永久删除单个文件。
    fn remove_file(&self, path: &Path) -> Result<(), CommandError> {
        std::fs::remove_file(path)?;
        Ok(())
    }

    /// 永久删除目录及其全部内容。
    fn remove_dir_all(&self, path: &Path) -> Result<(), CommandError> {
        std::fs::remove_dir_all(path)?;
        Ok(())
    }
}

/// 系统回收站策略：交给操作系统托管，用户可从回收站还原。
pub(crate) struct TrashRemover;

impl Remover for TrashRemover {
    /// 把单个文件移入系统回收站。
    fn remove_file(&self, path: &Path) -> Result<(), CommandError> {
        send_to_trash(path)
    }

    /// 把整个目录移入系统回收站；目录内容由系统 API 递归处理。
    fn remove_dir_all(&self, path: &Path) -> Result<(), CommandError> {
        send_to_trash(path)
    }
}

/// 在专用线程上执行回收站操作。
///
/// 为什么必须换线程：trash 的 Windows 实现按线程初始化 COM，初始化失败
/// 会直接 panic（见其 windows.rs 的 CoInitializer）。调用方来自 tokio
/// 工作线程，其 COM 套间模型不受我们控制，一旦被别的组件初始化过，
/// panic 会带崩整个命令。新建线程的 COM 必然未初始化，初始化一定成功；
/// 即使仍然 panic 也会被 join 捕获成普通错误返回前端。
fn send_to_trash(path: &Path) -> Result<(), CommandError> {
    let target = path.to_path_buf();
    let joined = std::thread::Builder::new()
        .name("quailcard-trash".to_string())
        .spawn(move || trash::delete(&target))
        .map_err(|error| {
            eprintln!("TRASH_SPAWN_ERROR(detail): {error}");
            CommandError::new(TRASH_UNAVAILABLE, "无法移到回收站")
        })?
        .join();
    match joined {
        Ok(Ok(())) => Ok(()),
        Ok(Err(error)) => {
            // 原始错误可能带本地绝对路径，只写后端日志，不返回前端。
            eprintln!("TRASH_ERROR(detail): {error}");
            Err(CommandError::new(TRASH_UNAVAILABLE, "无法移到回收站"))
        }
        Err(_) => {
            eprintln!("TRASH_PANIC(detail): 回收站线程异常退出");
            Err(CommandError::new(TRASH_UNAVAILABLE, "无法移到回收站"))
        }
    }
}

/// 按用户设置选择删除策略。
///
/// 返回静态引用：两种策略都是无状态单元结构体，无需每次分配。
pub(crate) fn remover_for(permanent: bool) -> &'static dyn Remover {
    static PERMANENT: PermanentRemover = PermanentRemover;
    static TRASH: TrashRemover = TrashRemover;
    if permanent {
        &PERMANENT
    } else {
        &TRASH
    }
}

impl VaultState {
    /// 删除笔记文件；删除方式由调用方选定的策略决定。
    pub fn delete_note(&self, relative: &str, remover: &dyn Remover) -> Result<(), CommandError> {
        let path = self.resolve(relative)?;
        if !path.is_file() {
            return Err(CommandError::new("NOTE_NOT_FOUND", "笔记文件不存在"));
        }
        remover.remove_file(&path)
    }

    /// 删除文件夹及其全部内容；删除方式由调用方选定的策略决定。
    pub fn delete_folder(&self, relative: &str, remover: &dyn Remover) -> Result<(), CommandError> {
        let path = self.resolve(relative)?;
        if !path.is_dir() {
            return Err(CommandError::new("FOLDER_NOT_FOUND", "文件夹不存在"));
        }
        remover.remove_dir_all(&path)
    }
}

#[cfg(test)]
#[path = "remove_tests.rs"]
mod remove_tests;
