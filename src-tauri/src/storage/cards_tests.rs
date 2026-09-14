//! 卡片存储的落盘与生命周期测试。

use super::super::testutil;
use super::*;

/// 创建测试卡片输入。
fn test_card(note_path: &str, kind: &str) -> CardInput {
    CardInput {
        id: None,
        note_path: note_path.to_string(),
        source_ref: None,
        source: None,
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
    let reopened = Storage::open(_config.path()).expect("重开存储失败");
    reopened
        .open_vault(vault.path(), &[])
        .await
        .expect("重开 Vault 失败");
    assert_eq!(
        reopened
            .list_note_cards("新/笔记.md")
            .await
            .expect("查询失败")
            .len(),
        1
    );
    assert!(reopened
        .list_note_cards("旧/笔记.md")
        .await
        .expect("查询失败")
        .is_empty());
}

#[tokio::test]
/// 来源往返持久化，普通编辑未传来源时保留旧数据。
async fn source_survives_save_edit_and_reload() {
    let (storage, config, vault) = testutil::test_storage().await;
    let mut input = test_card("测试/笔记.md", "qa");
    input.source_ref = Some("旧来源".into());
    input.source = Some(crate::models::CardSource {
        from: 12,
        to: 14,
        excerpt: "答案".into(),
        prefix: "前文".into(),
        suffix: "后文".into(),
    });
    let saved = storage.save_card(input).await.expect("保存失败");
    let mut edit = test_card("测试/笔记.md", "qa");
    edit.id = Some(saved.id);
    edit.front = "修改问题".into();
    storage.save_card(edit).await.expect("编辑失败");
    let reopened = Storage::open(config.path()).expect("重开存储失败");
    reopened
        .open_vault(vault.path(), &[])
        .await
        .expect("重开失败");
    let cards = reopened
        .list_note_cards("测试/笔记.md")
        .await
        .expect("读取失败");
    assert_eq!(cards[0].source_ref, "旧来源");
    assert_eq!(
        cards[0].source.as_ref().expect("来源应保留").excerpt,
        "答案"
    );
    assert_eq!(cards[0].source.as_ref().expect("来源应保留").from, 12);
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
