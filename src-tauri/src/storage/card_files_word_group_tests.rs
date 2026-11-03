//! 单词组卡片命名空间测试：.qword 落到 wordgroups/，路径回推与 Markdown 互不干扰。

use std::path::{Path, PathBuf};

use super::{card_relative_path, derive_note_path, CARDS_DIR_NAME, WORD_GROUPS_DIR_NAME};

#[test]
/// Markdown 卡片仍保持 .quailcard/<去 .md>.json 的原布局。
fn maps_markdown_cards_to_legacy_layout() {
    assert_eq!(
        card_relative_path("笔记.md").unwrap(),
        Path::new(CARDS_DIR_NAME).join("笔记.json")
    );
    assert_eq!(
        card_relative_path("目录/子/笔记.md").unwrap(),
        Path::new(CARDS_DIR_NAME).join("目录/子/笔记.json")
    );
}

#[test]
/// 单词组卡片进入 wordgroups/ 命名空间并保留子目录。
fn maps_word_group_cards_into_namespace() {
    assert_eq!(
        card_relative_path("单词.qword").unwrap(),
        Path::new(CARDS_DIR_NAME)
            .join(WORD_GROUPS_DIR_NAME)
            .join("单词.json")
    );
    assert_eq!(
        card_relative_path("课程/英语/单词.QWORD").unwrap(),
        Path::new(CARDS_DIR_NAME)
            .join(WORD_GROUPS_DIR_NAME)
            .join("课程/英语/单词.json")
    );
}

#[test]
/// 镜像路径回推：wordgroups/ 前缀还原成 .qword，其余仍是 .md。
fn derives_note_paths_from_card_files() {
    assert_eq!(derive_note_path("笔记.json"), "笔记.md");
    assert_eq!(derive_note_path("目录/笔记.json"), "目录/笔记.md");
    assert_eq!(derive_note_path("wordgroups/单词.json"), "单词.qword");
    assert_eq!(
        derive_note_path("wordgroups/课程/英语/单词.json"),
        "课程/英语/单词.qword"
    );
    // 只认命名空间前缀，普通目录里叫 wordgroups 尾段不受影响。
    assert_eq!(
        derive_note_path("目录/wordgroups/笔记.json"),
        "目录/wordgroups/笔记.md"
    );
}

#[test]
/// 卡片路径与笔记路径可以互为逆运算（去掉 .quailcard 前缀后）。
fn card_and_note_paths_roundtrip() {
    for note in [
        "笔记.md",
        "目录/笔记.md",
        "单词.qword",
        "课程/英语/单词.qword",
    ] {
        let card = card_relative_path(note).unwrap();
        let relative = card
            .strip_prefix(CARDS_DIR_NAME)
            .expect("卡片路径应位于 .quailcard 内")
            .to_string_lossy()
            .replace('\\', "/");
        assert_eq!(derive_note_path(&relative), note);
    }
}

#[test]
/// 非法路径（目录穿越）在计算卡片路径前就被拒绝。
fn rejects_unsafe_note_paths() {
    assert!(card_relative_path("../逃逸.md").is_err());
    assert!(card_relative_path("C:/逃逸.md").is_err());
}

#[test]
/// 命名空间常量与预期字面量一致，避免上下游对目录名理解不一。
fn exposes_stable_namespace_names() {
    assert_eq!(WORD_GROUPS_DIR_NAME, "wordgroups");
    let _: PathBuf = PathBuf::from(CARDS_DIR_NAME);
}
