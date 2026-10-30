use std::path::PathBuf;

use uuid::Uuid;

use super::attachment_actions::{attachment_file_hints, read_attachment_bytes, write_replacing};
use super::VaultState;

const PNG: &[u8] = b"\x89PNG\r\n\x1a\nminimal-test-payload";
const JPEG: &[u8] = &[0xff, 0xd8, 0xff, 0xe0, 1, 2, 3];
const WEBP: &[u8] = b"RIFF\x00\x00\x00\x00WEBPVP8 ";

/// 创建测试 Vault，并在离开作用域时清理。
struct TestVault(PathBuf);

impl TestVault {
    /// 使用 UUID 隔离并发测试目录。
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!("quailcard-actions-{}", Uuid::now_v7()));
        std::fs::create_dir(&path).unwrap();
        Self(path)
    }

    /// 构造已选择此测试目录的 VaultState。
    fn state(&self) -> VaultState {
        let state = VaultState::new();
        state.set_root(self.0.clone()).unwrap();
        state
    }
}

impl Drop for TestVault {
    /// 测试结束后尽力删除临时 Vault。
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[test]
/// 解析链必须把图片定位到 Vault 内的真实文件，供复制与另存复用。
fn resolves_existing_attachment_file() {
    let vault = TestVault::new();
    std::fs::create_dir_all(vault.0.join("attachments")).unwrap();
    std::fs::write(vault.0.join("attachments/a.png"), PNG).unwrap();
    std::fs::write(vault.0.join("note.md"), "# Note").unwrap();
    let state = vault.state();

    let path = state
        .attachment_file_path("note.md", "attachments/a.png")
        .unwrap();
    assert!(path.is_file());
    assert_eq!(
        state
            .read_attachment("note.md", "attachments/a.png")
            .unwrap(),
        PNG
    );
}

#[test]
/// 越过 Vault 根目录的来源必须在解析阶段被拒绝。
fn rejects_source_escaping_vault() {
    let vault = TestVault::new();
    std::fs::write(vault.0.join("note.md"), "# Note").unwrap();
    let state = vault.state();
    for source in [
        "../outside.png",
        "attachments/../../outside.png",
        "/etc/passwd",
        "C:/x.png",
    ] {
        assert!(
            state.attachment_file_path("note.md", source).is_err(),
            "accepted {source}"
        );
    }
}

#[test]
/// 笔记不存在时先报笔记错误，避免把附件错误暴露给不存在的引用。
fn reports_missing_note_before_attachment() {
    let vault = TestVault::new();
    let state = vault.state();
    let error = state
        .attachment_file_path("missing.md", "attachments/a.png")
        .unwrap_err();
    assert_eq!(error.code, "NOTE_NOT_FOUND");
}

#[test]
/// 图片文件缺失时给出专门的附件错误码。
fn reports_missing_attachment() {
    let vault = TestVault::new();
    std::fs::write(vault.0.join("note.md"), "# Note").unwrap();
    std::fs::create_dir_all(vault.0.join("attachments")).unwrap();
    let state = vault.state();
    let error = state
        .attachment_file_path("note.md", "attachments/gone.png")
        .unwrap_err();
    assert_eq!(error.code, "ATTACHMENT_NOT_FOUND");
}

#[test]
/// 目录不能被当作附件读取。
fn rejects_directory_as_attachment_source() {
    let vault = TestVault::new();
    std::fs::create_dir_all(vault.0.join("attachments")).unwrap();
    std::fs::write(vault.0.join("note.md"), "# Note").unwrap();
    let state = vault.state();
    let error = state
        .attachment_file_path("note.md", "attachments")
        .unwrap_err();
    assert_eq!(error.code, "ATTACHMENT_NOT_FOUND");
}

#[test]
/// 读取入口必须拦住超过 10 MiB 的附件，避免复制时把大文件读进内存。
fn rejects_oversized_attachment() {
    let vault = TestVault::new();
    let path = vault.0.join("big.png");
    let mut bytes = PNG.to_vec();
    bytes.resize(10 * 1024 * 1024 + 1, 0);
    std::fs::write(&path, &bytes).unwrap();

    let error = read_attachment_bytes(&path).unwrap_err();
    assert_eq!(error.code, "VALIDATION_ERROR");
}

#[test]
/// 另存提示的扩展名来自真实图片类型，而不是原始文件名。
fn hints_follow_detected_image_type() {
    let path = std::path::Path::new("/tmp/diagram.png");
    assert_eq!(
        attachment_file_hints(path, PNG).unwrap(),
        ("diagram".to_string(), "png")
    );
    assert_eq!(
        attachment_file_hints(path, JPEG).unwrap(),
        ("diagram".to_string(), "jpg")
    );
    assert_eq!(
        attachment_file_hints(path, WEBP).unwrap(),
        ("diagram".to_string(), "webp")
    );
}

#[test]
/// 非图片内容不提供另存提示，直接按校验错误返回。
fn hints_reject_non_image_content() {
    let path = std::path::Path::new("/tmp/diagram.png");
    let error = attachment_file_hints(path, b"plain text").unwrap_err();
    assert_eq!(error.code, "VALIDATION_ERROR");
}

#[test]
/// 另存必须覆盖同名目标，并且不残留临时文件。
fn export_overwrites_existing_target_atomically() {
    let vault = TestVault::new();
    let target = vault.0.join("saved.png");
    std::fs::write(&target, b"old").unwrap();

    write_replacing(&target, PNG).unwrap();
    assert_eq!(std::fs::read(&target).unwrap(), PNG);
    let leftovers: Vec<_> = std::fs::read_dir(&vault.0)
        .unwrap()
        .flatten()
        .map(|entry| entry.file_name().to_string_lossy().into_owned())
        .filter(|name| name.starts_with(".quailcard-save-"))
        .collect();
    assert!(leftovers.is_empty(), "left behind {leftovers:?}");
}

#[test]
/// 目标目录不存在时给出可读的校验错误，而不是把系统错误抛给用户。
fn export_rejects_missing_target_directory() {
    let vault = TestVault::new();
    let target = vault.0.join("missing/dir/saved.png");
    let error = write_replacing(&target, PNG).unwrap_err();
    assert_eq!(error.code, "VALIDATION_ERROR");
}
