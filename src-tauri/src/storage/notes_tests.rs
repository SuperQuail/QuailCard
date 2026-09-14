use super::notes::NoteIndex;

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
