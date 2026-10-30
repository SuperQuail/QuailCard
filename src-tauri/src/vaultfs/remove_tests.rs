//! 删除策略的委派与错误映射测试。

use std::path::{Path, PathBuf};
use std::sync::Mutex;

use uuid::Uuid;

use super::{remover_for, PermanentRemover, Remover, TrashRemover};
use crate::error::CommandError;
use crate::vaultfs::VaultState;

/// 记录调用路径的假策略；同时可模拟失败以验证错误向上传递。
#[derive(Default)]
struct RecordingRemover {
    /// 被请求删除的绝对路径，按调用顺序记录。
    calls: Mutex<Vec<PathBuf>>,
    /// 置位后所有删除请求都失败。
    fail: bool,
}

impl RecordingRemover {
    /// 取出已记录的调用，便于断言。
    fn calls(&self) -> Vec<PathBuf> {
        self.calls.lock().unwrap().clone()
    }

    /// 记录一次调用并按需返回错误。
    fn record(&self, path: &Path) -> Result<(), CommandError> {
        self.calls.lock().unwrap().push(path.to_path_buf());
        if self.fail {
            return Err(CommandError::new("BOOM", "删除失败"));
        }
        Ok(())
    }
}

impl Remover for RecordingRemover {
    /// 记录文件删除请求。
    fn remove_file(&self, path: &Path) -> Result<(), CommandError> {
        self.record(path)
    }

    /// 记录目录删除请求。
    fn remove_dir_all(&self, path: &Path) -> Result<(), CommandError> {
        self.record(path)
    }
}

/// 创建测试 Vault，离开作用域时清理。
struct TestVault(PathBuf);

impl TestVault {
    /// 用 UUID 隔离并发测试目录。
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!("quailcard-remove-{}", Uuid::now_v7()));
        std::fs::create_dir_all(&path).unwrap();
        Self(path)
    }

    /// 构造已选择此目录的 VaultState。
    fn state(&self) -> VaultState {
        let state = VaultState::new();
        state.set_root(self.0.clone()).unwrap();
        state
    }

    /// 在 Vault 内写入一个文件并返回其相对路径。
    fn write(&self, relative: &str) -> String {
        let path = self.0.join(relative);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).unwrap();
        }
        std::fs::write(&path, b"# t").unwrap();
        relative.to_string()
    }
}

impl Drop for TestVault {
    /// 测试结束后尽力删除临时目录。
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[test]
/// 删除笔记把经过根目录约束的绝对路径交给策略，而不是相对路径。
fn delegates_note_deletion_with_absolute_path() {
    let vault = TestVault::new();
    let relative = vault.write("英语/单词.md");
    let state = vault.state();
    let remover = RecordingRemover::default();

    state.delete_note(&relative, &remover).unwrap();

    let root = state.root().unwrap().unwrap();
    assert_eq!(remover.calls(), vec![root.join("英语/单词.md")]);
}

#[test]
/// 笔记不存在时不调用策略，避免把"文件已消失"误报成删除方式的问题。
fn rejects_missing_note_before_strategy() {
    let vault = TestVault::new();
    let state = vault.state();
    let remover = RecordingRemover::default();

    let error = state.delete_note("不存在.md", &remover).unwrap_err();

    assert_eq!(error.code, "NOTE_NOT_FOUND");
    assert!(remover.calls().is_empty());
}

#[test]
/// 目录穿越在净化阶段就被拒绝，不会触达任何删除策略。
fn rejects_traversal_before_strategy() {
    let vault = TestVault::new();
    let state = vault.state();
    let remover = RecordingRemover::default();

    assert!(state.delete_note("../外逃.md", &remover).is_err());
    assert!(state.delete_folder("../外逃", &remover).is_err());
    assert!(remover.calls().is_empty());
}

#[test]
/// 删除文件夹同样委派给策略，且策略收到的是绝对路径。
fn delegates_folder_deletion_with_absolute_path() {
    let vault = TestVault::new();
    vault.write("英语/单词.md");
    let state = vault.state();
    let remover = RecordingRemover::default();

    state.delete_folder("英语", &remover).unwrap();

    let root = state.root().unwrap().unwrap();
    assert_eq!(remover.calls(), vec![root.join("英语")]);
}

#[test]
/// 文件夹不存在时报 FOLDER_NOT_FOUND，而不是静默成功。
fn rejects_missing_folder() {
    let vault = TestVault::new();
    let state = vault.state();
    let remover = RecordingRemover::default();

    let error = state.delete_folder("不存在", &remover).unwrap_err();

    assert_eq!(error.code, "FOLDER_NOT_FOUND");
    assert!(remover.calls().is_empty());
}

#[test]
/// 策略失败时原样向上传递，命令层据此决定是否回滚卡片状态。
fn propagates_strategy_failure() {
    let vault = TestVault::new();
    let relative = vault.write("单词.md");
    let state = vault.state();
    let remover = RecordingRemover {
        calls: Mutex::default(),
        fail: true,
    };

    let error = state.delete_note(&relative, &remover).unwrap_err();

    assert_eq!(error.code, "BOOM");
}

#[test]
/// 永久删除策略真的会移除文件与目录。
fn permanent_remover_removes_file_and_directory() {
    let vault = TestVault::new();
    let file = vault.0.join("单词.md");
    let directory = vault.0.join("英语");
    std::fs::write(&file, b"# t").unwrap();
    std::fs::create_dir_all(directory.join("子目录")).unwrap();
    std::fs::write(directory.join("子目录/词.md"), b"# t").unwrap();

    PermanentRemover.remove_file(&file).unwrap();
    PermanentRemover.remove_dir_all(&directory).unwrap();

    assert!(!file.exists());
    assert!(!directory.exists());
}

#[test]
/// 设置开关映射到不同策略实例：开启永久删除不会走到回收站。
fn maps_setting_to_distinct_strategies() {
    let permanent = remover_for(true) as *const dyn Remover;
    let trashed = remover_for(false) as *const dyn Remover;

    assert!(!std::ptr::eq(permanent, trashed));
    assert!(std::ptr::eq(
        permanent,
        remover_for(true) as *const dyn Remover
    ));
}

#[test]
#[ignore = "会向本机回收站写入真实文件，仅在验证平台回收站实现时手动运行"]
/// 真实回收站冒烟：证明当前平台的回收站实现可用且文件确实离开原位置。
///
/// 默认忽略：CI 未必有回收站（容器、无桌面会话），运行还会在开发机留下
/// 回收站条目。改动删除相关代码后手动跑一次即可。
fn trashes_real_file() {
    let vault = TestVault::new();
    let file = vault.0.join("回收站冒烟.md");
    std::fs::write(&file, b"# t").unwrap();

    TrashRemover.remove_file(&file).unwrap();

    assert!(!file.exists());
}

#[test]
/// 回收站不可用时统一返回 TRASH_UNAVAILABLE，前端据此提示改为永久删除。
fn maps_trash_failure_to_unavailable() {
    let missing = std::env::temp_dir().join(format!("quailcard-缺-{}.md", Uuid::now_v7()));

    let error = TrashRemover.remove_file(&missing).unwrap_err();

    assert_eq!(error.code, "TRASH_UNAVAILABLE");
    assert_eq!(error.message, "无法移到回收站");
}
