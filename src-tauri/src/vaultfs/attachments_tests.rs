use std::path::{Path, PathBuf};

use base64::{engine::general_purpose, Engine as _};
use uuid::Uuid;

use super::VaultState;
use crate::models::{ImportNoteAttachmentInput, ReadNoteAttachmentInput, VaultConfig};

const PNG: &[u8] = b"\x89PNG\r\n\x1a\nminimal-test-payload";

/// 创建测试 Vault，并在离开作用域时清理。
struct TestVault(PathBuf);

impl TestVault {
    /// 使用 UUID 隔离并发测试目录。
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!("quailcard-{}", Uuid::now_v7()));
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

/// 构造签名有效且足够验证往返的 PNG 导入输入。
fn png_input(note_path: &str) -> ImportNoteAttachmentInput {
    ImportNoteAttachmentInput {
        note_path: note_path.to_string(),
        file_name: "diagram.png".to_string(),
        mime_type: "image/png".to_string(),
        data_base64: general_purpose::STANDARD.encode(PNG),
    }
}

#[test]
/// 不同 Vault 分别读取默认值并持久化各自配置。
fn config_is_defaulted_persisted_and_isolated() {
    let first = TestVault::new();
    let second = TestVault::new();
    let first_state = first.state();
    let second_state = second.state();

    assert_eq!(first_state.get_config().unwrap(), VaultConfig::default());
    assert!(first.0.join(".quailcard/config.json").is_file());
    first_state.set_attachment_folder("media/images").unwrap();
    second_state.set_attachment_folder("assets").unwrap();

    assert_eq!(
        first.state().get_config().unwrap().attachment_folder,
        "media/images"
    );
    assert_eq!(
        second.state().get_config().unwrap().attachment_folder,
        "assets"
    );
    assert!(first.0.join(".quailcard/config.json").is_file());
}

#[test]
/// 配置目录拒绝特殊元数据目录和所有穿越或平台前缀写法。
fn config_rejects_unsafe_folders() {
    let vault = TestVault::new();
    let state = vault.state();
    for folder in [
        "",
        ".",
        "..",
        ".quailcard",
        "a/.quailcard",
        "../outside",
        "C:/temp",
        "a\\b",
        "/root",
        "a//b",
    ] {
        assert!(
            state.set_attachment_folder(folder).is_err(),
            "accepted {folder}"
        );
    }
}

#[test]
/// 嵌套笔记导入后返回标准相对路径，并可按该路径原样读取。
fn imported_image_roundtrips_from_nested_note() {
    let vault = TestVault::new();
    std::fs::create_dir_all(vault.0.join("notes/deep")).unwrap();
    std::fs::write(vault.0.join("notes/deep/topic.md"), "# Topic").unwrap();
    let state = vault.state();

    let imported = state
        .import_note_attachment(png_input("notes/deep/topic.md"))
        .unwrap();
    assert!(imported
        .markdown_path
        .starts_with("../../attachments/diagram-"));
    assert!(imported.markdown_path.ends_with(".png"));

    let image = state
        .read_note_attachment(ReadNoteAttachmentInput {
            note_path: "notes/deep/topic.md".to_string(),
            source: imported.markdown_path,
        })
        .unwrap();
    assert_eq!(image.mime_type, "image/png");
    assert_eq!(
        general_purpose::STANDARD.decode(image.data_base64).unwrap(),
        PNG
    );
}

#[test]
/// 导入拒绝 MIME 欺骗、无效 Base64 和危险文件名。
fn import_rejects_invalid_payloads() {
    let vault = TestVault::new();
    std::fs::write(vault.0.join("note.md"), "# Note").unwrap();
    let state = vault.state();

    let mut wrong_magic = png_input("note.md");
    wrong_magic.mime_type = "image/jpeg".to_string();
    assert!(state.import_note_attachment(wrong_magic).is_err());
    let mut invalid_base64 = png_input("note.md");
    invalid_base64.data_base64 = "not base64".to_string();
    assert!(state.import_note_attachment(invalid_base64).is_err());
    let mut unsafe_name = png_input("note.md");
    unsafe_name.file_name = "../image.png".to_string();
    assert!(state.import_note_attachment(unsafe_name).is_err());
}

#[test]
/// 导入文件名会移除空格和 Markdown 结构字符，保证返回链接无需额外转义。
fn import_normalizes_markdown_unsafe_file_name() {
    let vault = TestVault::new();
    std::fs::write(vault.0.join("note.md"), "# Note").unwrap();
    let state = vault.state();
    let mut input = png_input("note.md");
    input.file_name = "my diagram (final).png".to_string();

    let imported = state.import_note_attachment(input).unwrap();

    assert!(imported
        .markdown_path
        .starts_with("attachments/my-diagram-final-"));
    assert!(!imported.markdown_path.contains([' ', '(', ')']));
}

#[test]
/// Markdown 读取允许受控父目录，但拒绝越界、URL 与查询片段。
fn read_rejects_unsafe_sources_and_bad_magic() {
    let vault = TestVault::new();
    std::fs::create_dir_all(vault.0.join("notes")).unwrap();
    std::fs::write(vault.0.join("notes/note.md"), "# Note").unwrap();
    std::fs::write(vault.0.join("bad.png"), "not an image").unwrap();
    let state = vault.state();

    for source in [
        "../../outside.png",
        "https://example.com/a.png",
        "C:/a.png",
        "\\\\server\\a.png",
        "../bad.png?x=1",
        "../bad.png#x",
    ] {
        assert!(
            state
                .read_note_attachment(ReadNoteAttachmentInput {
                    note_path: "notes/note.md".to_string(),
                    source: source.to_string(),
                })
                .is_err(),
            "accepted {source}"
        );
    }
    assert!(state
        .read_note_attachment(ReadNoteAttachmentInput {
            note_path: "notes/note.md".to_string(),
            source: "../bad.png".to_string(),
        })
        .is_err());
    assert!(state
        .read_note_attachment(ReadNoteAttachmentInput {
            note_path: "notes/missing.md".to_string(),
            source: "../bad.png".to_string(),
        })
        .is_err());
    assert!(Path::new(&vault.0).is_dir());
}
