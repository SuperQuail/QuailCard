//! 文档类型识别与标题推导的单元测试。

use super::{strip_known_extension, title_from_path, DocumentKind};

#[test]
/// 扩展名识别大小写不敏感，未知扩展名与隐藏文件不算文档。
fn identifies_document_kinds() {
    assert_eq!(
        DocumentKind::of_path("笔记.md"),
        Some(DocumentKind::Markdown)
    );
    assert_eq!(
        DocumentKind::of_path("目录/笔记.MD"),
        Some(DocumentKind::Markdown)
    );
    assert_eq!(
        DocumentKind::of_path("单词.qword"),
        Some(DocumentKind::WordGroup)
    );
    assert_eq!(
        DocumentKind::of_path("目录/子目录/单词.QWord"),
        Some(DocumentKind::WordGroup)
    );
    assert_eq!(
        DocumentKind::of_path("a.b.qword"),
        Some(DocumentKind::WordGroup)
    );
    assert_eq!(DocumentKind::of_path("图片.png"), None);
    assert_eq!(DocumentKind::of_path("没有扩展名"), None);
    assert_eq!(DocumentKind::of_path(".md"), None);
    assert_eq!(DocumentKind::of_path(""), None);
}

#[test]
/// 标题推导只剥已知扩展名，未知扩展名属于标题的一部分。
fn derives_titles_from_paths() {
    assert_eq!(title_from_path("目录/单词.qword"), "单词");
    assert_eq!(title_from_path("目录/笔记.MD"), "笔记");
    assert_eq!(title_from_path("a.b.qword"), "a.b");
    assert_eq!(title_from_path("图片.png"), "图片.png");
    assert_eq!(title_from_path("没有扩展名"), "没有扩展名");
    assert_eq!(title_from_path("a\\b\\笔记.md"), "笔记");
}

#[test]
/// 已知扩展名剥离不区分大小写，且不会把纯点开头的名字剥空。
fn strips_only_known_extensions() {
    assert_eq!(strip_known_extension("笔记.md"), "笔记");
    assert_eq!(strip_known_extension("笔记.Md"), "笔记");
    assert_eq!(strip_known_extension("单词.qword"), "单词");
    assert_eq!(strip_known_extension("单词.QWORD"), "单词");
    assert_eq!(strip_known_extension("归档.tar.gz"), "归档.tar.gz");
    assert_eq!(strip_known_extension(".md"), ".md");
    assert_eq!(strip_known_extension("无扩展名"), "无扩展名");
}

#[test]
/// 文档类型的规范扩展名与 Markdown 判定保持一致。
fn exposes_extensions_and_markdown_flag() {
    assert_eq!(DocumentKind::Markdown.extension(), ".md");
    assert_eq!(DocumentKind::WordGroup.extension(), ".qword");
    assert!(DocumentKind::Markdown.is_markdown());
    assert!(!DocumentKind::WordGroup.is_markdown());
}
