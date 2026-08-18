//! 卡片存储的落盘与生命周期测试。

use super::super::testutil;
use super::*;
use crate::models::GeneratedCard;
use std::collections::HashMap;

/// 创建测试卡片输入。
fn test_card(note_path: &str, kind: &str) -> CardInput {
    CardInput {
        id: None,
        note_path: note_path.to_string(),
        source_ref: None,
        kind: kind.to_string(),
        front: "问题".to_string(),
        back: "答案".to_string(),
        detail: None,
        example: None,
        aliases: Vec::new(),
        rubric: Vec::new(),
    }
}

#[tokio::test]
/// 保存卡片会落盘镜像文件并可重新加载。
async fn save_card_persists_mirror_file() {
    let (storage, _config, vault) = testutil::test_storage().await;
    let saved = storage
        .save_card(test_card("测试/笔记.md", "qa"))
        .await
        .expect("保存卡片失败");
    assert_eq!(saved.position, 0);
    let mirror = vault.path().join(".quailcard/测试/笔记.json");
    assert!(mirror.is_file(), "镜像卡片文件应已写入");
    let cards = storage
        .list_note_cards("测试/笔记.md")
        .await
        .expect("查询卡片失败");
    assert_eq!(cards.len(), 1);
    assert_eq!(cards[0].id, saved.id);
    assert_eq!(cards[0].scheduler_phase, "new");
    // 换一个存储实例模拟重启：数据必须能从磁盘恢复。
    let reopened = Storage::open(_config.path()).expect("重开存储失败");
    reopened
        .open_vault(vault.path(), &[])
        .await
        .expect("重开 Vault 失败");
    let reloaded = reopened
        .list_note_cards("测试/笔记.md")
        .await
        .expect("重载卡片失败");
    assert_eq!(reloaded.len(), 1);
    assert_eq!(reloaded[0].id, saved.id);
}

#[tokio::test]
/// 删除最后一张卡片时镜像文件一并移除。
async fn delete_card_removes_empty_mirror_file() {
    let (storage, _config, vault) = testutil::test_storage().await;
    let saved = storage
        .save_card(test_card("测试/笔记.md", "qa"))
        .await
        .expect("保存卡片失败");
    storage.delete_card(&saved.id).await.expect("删除卡片失败");
    let mirror = vault.path().join(".quailcard/测试/笔记.json");
    assert!(!mirror.exists(), "空镜像文件应被移除");
    assert!(storage
        .list_note_cards("测试/笔记.md")
        .await
        .expect("查询卡片失败")
        .is_empty());
}

#[tokio::test]
/// 采纳草稿只写入正反面完整的卡片。
async fn adopt_cards_skips_incomplete_drafts() {
    let (storage, _config, _vault) = testutil::test_storage().await;
    let draft = |front: &str, back: &str| GeneratedCard {
        fields: HashMap::from([
            ("front".to_string(), front.to_string()),
            ("back".to_string(), back.to_string()),
            ("aliases".to_string(), "词一、词二".to_string()),
        ]),
    };
    let count = storage
        .adopt_cards(&AdoptCardsInput {
            note_path: "测试/笔记.md".to_string(),
            kind: "qa".to_string(),
            cards: vec![draft("一", "答一"), draft("", "缺正面")],
        })
        .await
        .expect("采纳草稿失败");
    assert_eq!(count, 1);
    let cards = storage
        .list_note_cards("测试/笔记.md")
        .await
        .expect("查询卡片失败");
    assert_eq!(cards.len(), 1);
    assert_eq!(cards[0].aliases, ["词一", "词二"]);
}

#[tokio::test]
/// 重命名笔记后镜像文件随路径移动且卡片可查。
async fn rename_note_moves_mirror_files() {
    let (storage, _config, vault) = testutil::test_storage().await;
    storage
        .save_card(test_card("旧/笔记.md", "qa"))
        .await
        .expect("保存卡片失败");
    storage
        .rename_note_paths("旧/笔记.md", "新/笔记.md")
        .await
        .expect("重命名失败");
    assert!(vault.path().join(".quailcard/新/笔记.json").is_file());
    assert!(!vault.path().join(".quailcard/旧/笔记.json").exists());
    assert_eq!(
        storage
            .list_note_cards("新/笔记.md")
            .await
            .expect("查询卡片失败")
            .len(),
        1
    );
}

#[tokio::test]
/// 非法卡片类型被校验拒绝。
async fn rejects_invalid_card_kind() {
    let (storage, _config, _vault) = testutil::test_storage().await;
    let error = storage
        .save_card(test_card("测试/笔记.md", "cloze"))
        .await
        .expect_err("非法类型应被拒绝");
    assert_eq!(error.code, "VALIDATION_ERROR");
}
