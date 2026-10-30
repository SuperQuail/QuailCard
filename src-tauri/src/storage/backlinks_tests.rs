//! 反链查询的测试：解析一致的链接才算引用，行号与片段取自原文。

use crate::storage::testutil;

/// 三个反引号的围栏：用转义避免正文里出现裸标记。
const FENCE: &str = "\u{60}\u{60}\u{60}";

#[tokio::test]
/// 指向目标笔记的链接会产出一条反链，行号与片段来自链接所在行。
async fn reports_backlinks_with_line_and_snippet() {
    let (storage, _config, _vault) = testutil::test_storage().await;
    storage
        .upsert_note_index("目标.md", "# 目标", 1)
        .await
        .unwrap();
    storage
        .upsert_note_index("引用.md", "第一行\n看 [[目标]] 与 [[别处]]\n", 1)
        .await
        .unwrap();
    let links = storage.backlinks("目标.md").await.unwrap();
    assert_eq!(links.len(), 1);
    assert_eq!(links[0].source_path, "引用.md");
    assert_eq!(links[0].line, 2);
    assert!(links[0].snippet.contains("[[目标]]"));
}

#[tokio::test]
/// 别名与路径写法同样能建立反链，规则与编辑器一致。
async fn resolves_aliases_and_paths() {
    let (storage, _config, _vault) = testutil::test_storage().await;
    storage
        .upsert_note_index("目录/长名字.md", "---\naliases: [短名]\n---\n# 长名字", 1)
        .await
        .unwrap();
    storage
        .upsert_note_index("引用.md", "[[短名]] 与 [[目录/长名字]]", 1)
        .await
        .unwrap();
    let links = storage.backlinks("目录/长名字.md").await.unwrap();
    // 同一行出现两处链接只保留一条预览。
    assert_eq!(links.len(), 1);
    assert_eq!(links[0].line, 1);
}

#[tokio::test]
/// 指向不存在笔记的链接、代码块里的链接都不产生反链。
async fn ignores_unresolved_and_code_links() {
    let (storage, _config, _vault) = testutil::test_storage().await;
    storage
        .upsert_note_index("目标.md", "# 目标", 1)
        .await
        .unwrap();
    let content = format!("[[不存在的笔记]]\n{FENCE}\n[[目标]]\n{FENCE}\n");
    storage
        .upsert_note_index("引用.md", &content, 1)
        .await
        .unwrap();
    assert!(storage.backlinks("目标.md").await.unwrap().is_empty());
}

#[tokio::test]
/// 反链按来源标题排序，同一篇笔记的多行按行号升序。
async fn sorts_by_title_then_line() {
    let (storage, _config, _vault) = testutil::test_storage().await;
    storage
        .upsert_note_index("目标.md", "# 目标", 1)
        .await
        .unwrap();
    storage
        .upsert_note_index("乙.md", "[[目标]]\n[[目标]]\n", 1)
        .await
        .unwrap();
    storage
        .upsert_note_index("甲.md", "\n[[目标]]\n", 1)
        .await
        .unwrap();
    let links = storage.backlinks("目标.md").await.unwrap();
    let order: Vec<(String, usize)> = links
        .iter()
        .map(|link| (link.source_path.clone(), link.line))
        .collect();
    assert_eq!(
        order,
        [
            ("乙.md".to_string(), 1),
            ("乙.md".to_string(), 2),
            ("甲.md".to_string(), 2)
        ]
    );
}
