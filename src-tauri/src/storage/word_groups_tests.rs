//! 单词组卡片物化测试：增删改、复习保留、草稿页不删卡、跨笔记跳过与对账。

use super::*;
use crate::{
    models::{ReviewRating, SubmitReviewInput},
    storage::testutil,
    word_group::{WordGroupDocument, WordPage, FORMAT_VERSION},
};

/// 构造一个完整页；测试按需覆盖音标与例句。
fn page(id: &str, word: &str, meaning: &str) -> WordPage {
    WordPage {
        id: id.to_string(),
        word: word.to_string(),
        meaning: meaning.to_string(),
        ..WordPage::default()
    }
}

/// 构造一个合法文档。
fn document(words: Vec<WordPage>) -> WordGroupDocument {
    WordGroupDocument {
        format_version: FORMAT_VERSION,
        words,
    }
}

/// 提交一次复习，验证卡片历史在同步后保留。
async fn review(storage: &Storage, card_id: &str) {
    storage
        .submit_review(SubmitReviewInput {
            card_id: card_id.to_string(),
            rating: ReviewRating::Good,
            expected_version: 0,
            idempotency_key: format!("key-{card_id}"),
        })
        .await
        .expect("提交复习失败");
}

/// 拼接单词组卡片文件的预期绝对路径，独立于实现代码验证命名空间。
fn card_file(root: &std::path::Path, note_path: &str) -> std::path::PathBuf {
    let stem = note_path.trim_end_matches(".qword");
    root.join(".quailcard")
        .join("wordgroups")
        .join(format!("{stem}.json"))
}

#[tokio::test]
/// 完整页物化为 vocabulary 卡片，字段映射与 position 顺序符合契约。
async fn materializes_complete_pages_into_vocabulary_cards() {
    let (storage, _config, vault) = testutil::test_storage().await;
    let note = "课程/单词.qword";
    let mut first = page("p1", "ephemeral", "短暂的");
    first.phonetic = "/iˈfemərəl/".to_string();
    first.example = "an ephemeral joy".to_string();
    let third = page("p3", "", "");
    let changed = storage
        .sync_word_group_cards(
            note,
            &document(vec![first, page("p2", "durable", "持久的"), third]),
        )
        .unwrap();
    assert!(changed);

    let cards = storage.list_note_cards(note).await.unwrap();
    assert_eq!(cards.len(), 2, "草稿页不生成卡片");
    assert_eq!(cards[0].id, "p1");
    assert_eq!(cards[0].kind, "vocabulary");
    assert_eq!(cards[0].front, "短暂的");
    assert_eq!(cards[0].back, "ephemeral");
    assert_eq!(cards[0].detail, "/iˈfemərəl/");
    assert_eq!(cards[0].example, "an ephemeral joy");
    assert!(cards[0].aliases.is_empty() && cards[0].rubric_points.is_empty());
    assert!(cards[0].source.is_none() && cards[0].source_ref.is_empty());
    assert_eq!(cards[0].position, 0);
    assert_eq!(cards[1].position, 1);
    // 卡片落在 wordgroups 命名空间，且不污染 Markdown 镜像目录。
    assert!(card_file(vault.path(), note).is_file());
    assert!(!vault.path().join(".quailcard/课程/单词.json").exists());
}

#[tokio::test]
/// 内容未变化时第二次同步不写盘（返回 false）。
async fn second_sync_without_changes_reports_unchanged() {
    let (storage, _config, _vault) = testutil::test_storage().await;
    let document = document(vec![page("p1", "ephemeral", "短暂的")]);
    assert!(storage
        .sync_word_group_cards("组.qword", &document)
        .unwrap());
    assert!(!storage
        .sync_word_group_cards("组.qword", &document)
        .unwrap());
}

#[tokio::test]
/// 草稿页不新建卡片，但不删除已有同 id 卡片（保留复习历史）。
async fn draft_page_keeps_existing_card_and_history() {
    let (storage, _config, _vault) = testutil::test_storage().await;
    let note = "组.qword";
    storage
        .sync_word_group_cards(note, &document(vec![page("p1", "ephemeral", "短暂的")]))
        .unwrap();
    review(&storage, "p1").await;

    // 编辑中间态：单词暂时被清空，页面变成草稿。
    let changed = storage
        .sync_word_group_cards(note, &document(vec![page("p1", "", "短暂的")]))
        .unwrap();
    assert!(!changed, "草稿页原样保留时不应写盘");
    let cards = storage.list_note_cards(note).await.unwrap();
    assert_eq!(cards.len(), 1);
    assert_eq!(cards[0].back, "ephemeral", "已有卡片内容不被草稿覆盖");
    assert_eq!(cards[0].total_reviews, 1);
}

#[tokio::test]
/// 页面 id 消失才删卡：连同复习历史一并移除，其余卡片保留。
async fn removed_page_deletes_card_and_history() {
    let (storage, _config, vault) = testutil::test_storage().await;
    let note = "组.qword";
    storage
        .sync_word_group_cards(
            note,
            &document(vec![
                page("p1", "ephemeral", "短暂的"),
                page("p2", "durable", "持久的"),
            ]),
        )
        .unwrap();
    review(&storage, "p2").await;

    let changed = storage
        .sync_word_group_cards(note, &document(vec![page("p1", "ephemeral", "短暂的")]))
        .unwrap();
    assert!(changed);
    let cards = storage.list_note_cards(note).await.unwrap();
    assert_eq!(cards.len(), 1);
    assert_eq!(cards[0].id, "p1");
    assert!(
        storage.get_review_progress("p2").await.is_err(),
        "被删卡片不应还能查询复习进度"
    );
    // 磁盘文件同步收缩到 1 张卡片。
    let raw = std::fs::read_to_string(card_file(vault.path(), note)).unwrap();
    assert!(!raw.contains("\"p2\""), "卡片文件不应残留已删除 id");
}

#[tokio::test]
/// 更新完整页保留 review/history/createdAt，只刷新内容字段。
async fn update_preserves_review_history_and_created_at() {
    let (storage, _config, _vault) = testutil::test_storage().await;
    let note = "组.qword";
    storage
        .sync_word_group_cards(note, &document(vec![page("p1", "ephemeral", "短暂的")]))
        .unwrap();
    review(&storage, "p1").await;
    let before = storage.inner.cards.cards_of(note);
    assert_eq!(before.len(), 1);

    let mut updated = page("p1", "ephemeral", "短暂的（改）");
    updated.phonetic = "/x/".to_string();
    assert!(storage
        .sync_word_group_cards(note, &document(vec![updated]))
        .unwrap());

    let after = storage.inner.cards.cards_of(note);
    assert_eq!(after[0].front, "短暂的（改）");
    assert_eq!(after[0].detail, "/x/");
    assert_eq!(
        after[0].created_at, before[0].created_at,
        "创建时间必须保留"
    );
    assert_eq!(after[0].review.total_reviews, 1, "调度状态必须保留");
    assert_eq!(after[0].history.len(), 1, "复习历史必须保留");
}

#[tokio::test]
/// 跨笔记相同 id 直接跳过并记日志，不抢占原卡片。
async fn cross_note_card_id_is_skipped() {
    let (storage, _config, _vault) = testutil::test_storage().await;
    storage
        .sync_word_group_cards(
            "甲.qword",
            &document(vec![page("p1", "ephemeral", "短暂的")]),
        )
        .unwrap();
    let changed = storage
        .sync_word_group_cards("乙.qword", &document(vec![page("p1", "durable", "持久的")]))
        .unwrap();
    assert!(!changed, "跨笔记 id 不应在乙组生成卡片");
    assert!(storage
        .list_note_cards("乙.qword")
        .await
        .unwrap()
        .is_empty());
    let owner_cards = storage.list_note_cards("甲.qword").await.unwrap();
    assert_eq!(owner_cards.len(), 1);
    assert_eq!(owner_cards[0].back, "ephemeral");
}

#[tokio::test]
/// 清空页面后卡片文件一并移除，内存索引不再有该笔记。
async fn empty_document_removes_card_file() {
    let (storage, _config, vault) = testutil::test_storage().await;
    let note = "测试/单词.qword";
    storage
        .sync_word_group_cards(note, &document(vec![page("p1", "ephemeral", "短暂的")]))
        .unwrap();
    assert!(card_file(vault.path(), note).is_file());
    assert!(storage
        .sync_word_group_cards(note, &document(Vec::new()))
        .unwrap());
    assert!(storage.list_note_cards(note).await.unwrap().is_empty());
    assert!(
        !card_file(vault.path(), note).exists(),
        "空卡片文件应被删除"
    );
}

#[tokio::test]
/// 单词组改名/移动后卡片镜像跟随路径搬进新命名空间，旧文件不残留。
async fn rename_moves_word_group_card_files() {
    let (storage, _config, vault) = testutil::test_storage().await;
    storage
        .sync_word_group_cards(
            "旧/组.qword",
            &document(vec![page("p1", "ephemeral", "短暂的")]),
        )
        .unwrap();
    storage
        .rename_note_paths("旧/组.qword", "新/组.qword")
        .await
        .unwrap();
    assert!(!card_file(vault.path(), "旧/组.qword").exists());
    assert!(card_file(vault.path(), "新/组.qword").is_file());
    let cards = storage.list_note_cards("新/组.qword").await.unwrap();
    assert_eq!(cards.len(), 1);
    assert_eq!(cards[0].id, "p1");
}

#[tokio::test]
/// 打开 Vault 时对账：合法单词组补齐卡片，损坏文件只跳过、仍出现在笔记列表。
async fn reconcile_materializes_valid_and_skips_invalid() {
    let (storage, _config, vault) = testutil::test_storage().await;
    let good = word_group::serialize(&document(vec![page("p1", "ephemeral", "短暂的")])).unwrap();
    std::fs::write(vault.path().join("合法.qword"), &good).unwrap();
    std::fs::write(vault.path().join("损坏.qword"), "不是 JSON").unwrap();
    let files = vec![
        ("合法.qword".to_string(), good, 1),
        ("损坏.qword".to_string(), "不是 JSON".to_string(), 2),
    ];
    storage.open_vault(vault.path(), &files).await.unwrap();

    let cards = storage.list_note_cards("合法.qword").await.unwrap();
    assert_eq!(cards.len(), 1);
    assert_eq!(cards[0].back, "ephemeral");
    assert!(storage
        .list_note_cards("损坏.qword")
        .await
        .unwrap()
        .is_empty());
    let notes = storage.list_notes().await.unwrap();
    let paths: Vec<&str> = notes.iter().map(|note| note.path.as_str()).collect();
    assert!(paths.contains(&"合法.qword"));
    assert!(paths.contains(&"损坏.qword"), "损坏的单词组仍应列出");
}
