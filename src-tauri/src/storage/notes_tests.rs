use super::notes::NoteIndex;
use crate::vaultfs::documents::DocumentKind;

/// 合法的单词组 JSON 夹具：两页，一页完整、一页缺例句。
const WORD_GROUP: &str = r#"{"formatVersion":1,"words":[{"id":"p1","word":"ephemeral","phonetic":"/iˈfemərəl/","meaning":"短暂的","example":"an ephemeral joy"},{"id":"p2","word":"durable","meaning":"持久的"}]}"#;

#[test]
/// 改名后立即更新显示标题与排序，不依赖重新扫描磁盘。
fn rename_updates_title_and_order() {
    let index = NoteIndex::default();
    index.rebuild(&[
        ("a.md".into(), "正文".into(), 1),
        ("b.md".into(), "".into(), 2),
    ]);
    index.rename_prefix("a.md", "z.md");
    let entries = index.snapshot();
    assert_eq!(entries[0].path, "b.md");
    assert_eq!(entries[1].title, "z");
    assert_eq!(entries[1].content, "正文");
}

#[test]
/// 单词组索引：不解析标签/别名/链接，正文置空，搜索文本由单词、释义与例句派生。
fn indexes_word_group_documents() {
    let index = NoteIndex::default();
    index.rebuild(&[
        ("笔记.md".into(), "# 标签 #测试\n[[单词]]".into(), 1),
        ("单词.qword".into(), WORD_GROUP.into(), 2),
    ]);
    let entries = index.snapshot();
    let group = entries
        .iter()
        .find(|entry| entry.path == "单词.qword")
        .expect("单词组应进入索引");
    assert_eq!(group.kind, DocumentKind::WordGroup);
    assert_eq!(group.title, "单词");
    assert!(group.tags.is_empty() && group.aliases.is_empty() && group.links.is_empty());
    assert!(
        group.content.is_empty(),
        "单词组正文不应被当作 Markdown 内容"
    );
    let text = group.search_text.as_deref().expect("单词组应有搜索文本");
    assert!(text.contains("ephemeral"));
    assert!(text.contains("短暂的"));
    assert!(text.contains("an ephemeral joy"));
    assert!(!text.contains("iˈfemərəl"), "音标不进入搜索文本");

    let markdown = entries
        .iter()
        .find(|entry| entry.path == "笔记.md")
        .expect("Markdown 应进入索引");
    assert_eq!(markdown.kind, DocumentKind::Markdown);
    assert!(markdown.search_text.is_none());
    assert_eq!(markdown.tags, ["测试"]);
}

#[test]
/// 损坏的单词组不阻断索引：仍然列出，只是搜索文本为空。
fn tolerates_broken_word_group() {
    let index = NoteIndex::default();
    index.rebuild(&[("坏.qword".into(), "不是 JSON".into(), 1)]);
    let entries = index.snapshot();
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].kind, DocumentKind::WordGroup);
    assert_eq!(entries[0].search_text.as_deref(), Some(""));
}

#[test]
/// 文件夹改名仅影响路径边界内的后代，保留笔记标题。
fn rename_folder_preserves_children() {
    let index = NoteIndex::default();
    index.rebuild(&[
        ("old/sub/a.md".into(), "".into(), 1),
        ("older/b.md".into(), "".into(), 2),
    ]);
    index.rename_prefix("old", "new");
    let entries = index.snapshot();
    assert_eq!(entries[0].path, "new/sub/a.md");
    assert_eq!(entries[0].title, "a");
    assert_eq!(entries[1].path, "older/b.md");
}
