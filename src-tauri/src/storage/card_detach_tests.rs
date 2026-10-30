//! 卡片软分离测试：分离、隔离区跳过、还原接回与超期清理。

use std::{
    path::{Path, PathBuf},
    time::{Duration, SystemTime},
};

use super::*;
use crate::{
    models::{CardInput, ReviewRating, SubmitReviewInput},
    storage::{testutil, Storage},
};

/// 构造测试卡片输入；同一笔记的卡片只需最小合法字段。
fn test_card(note_path: &str) -> CardInput {
    CardInput {
        id: None,
        note_path: note_path.to_string(),
        source_ref: None,
        source: None,
        kind: "qa".to_string(),
        front: "问题".to_string(),
        back: "答案".to_string(),
        detail: None,
        example: None,
        aliases: Vec::new(),
        rubric: Vec::new(),
    }
}

/// 在 Vault 中写入笔记文件，模拟笔记存在；还原场景也用它重建同名笔记。
fn write_note(vault: &Path, note_path: &str) {
    let path = vault.join(note_path);
    std::fs::create_dir_all(path.parent().expect("笔记应有父目录")).expect("创建笔记目录失败");
    std::fs::write(&path, "# 笔记\n").expect("写入笔记失败");
}

/// 按规格的目录布局直接拼出隔离文件绝对路径，避免复用实现内部工具。
fn detached_file_path(vault: &Path, note_path: &str) -> PathBuf {
    let inner = note_path.trim_end_matches(".md");
    vault
        .join(CARDS_DIR_NAME)
        .join(DETACHED_DIR_NAME)
        .join(format!("{inner}.json"))
}

/// 为指定笔记保存一张卡片并返回卡片 ID。
async fn save_card(storage: &Storage, note_path: &str) -> String {
    storage
        .save_card(test_card(note_path))
        .await
        .expect("保存卡片失败")
        .id
}

/// 走 CardStore 的分离入口，与命令层最终调用的路径一致。
fn detach_note(storage: &Storage, note_path: &str) {
    storage
        .inner
        .cards
        .detach_note_paths(note_path)
        .expect("分离卡片失败");
}

/// 构造一份可解析的隔离卡片文件（只保留接回所需的最小字段）。
fn write_detached_file(path: &Path, note_path: Option<&str>) {
    std::fs::create_dir_all(path.parent().expect("隔离文件应有父目录")).expect("创建隔离目录失败");
    let body = match note_path {
        Some(note_path) => format!(
            r#"{{"formatVersion":1,"notePath":"{note_path}","cards":[{{"id":"card-1","front":"问","back":"答"}}]}}"#
        ),
        None => {
            r#"{"formatVersion":1,"cards":[{"id":"card-1","front":"问","back":"答"}]}"#.to_string()
        }
    };
    std::fs::write(path, body).expect("写入隔离文件失败");
}

#[tokio::test]
/// 分离后卡片立即离开内存索引与复习队列，磁盘上出现隔离文件。
async fn detach_removes_cards_from_active_state() {
    let (storage, _config, vault) = testutil::test_storage().await;
    let note = "测试/笔记.md";
    write_note(vault.path(), note);
    let card_id = save_card(&storage, note).await;
    assert_eq!(
        storage
            .get_review_queue(Some(note), true)
            .await
            .expect("查询队列失败")
            .len(),
        1
    );

    detach_note(&storage, note);

    assert!(storage
        .list_note_cards(note)
        .await
        .expect("查询卡片失败")
        .is_empty());
    assert!(storage
        .get_review_queue(None, true)
        .await
        .expect("查询队列失败")
        .is_empty());
    assert_eq!(
        storage
            .get_study_stats()
            .await
            .expect("查询统计失败")
            .total_cards,
        0
    );
    assert!(
        storage.get_review_progress(&card_id).await.is_err(),
        "分离后卡片不应再可查"
    );
    assert!(
        detached_file_path(vault.path(), note).is_file(),
        "隔离文件应已写入"
    );
}

#[tokio::test]
/// 分离后 active 镜像文件与它留下的空目录都被清理。
async fn detach_removes_active_file_and_empty_dirs() {
    let (storage, _config, vault) = testutil::test_storage().await;
    let note = "测试/笔记.md";
    write_note(vault.path(), note);
    save_card(&storage, note).await;

    detach_note(&storage, note);

    let cards_dir = vault.path().join(CARDS_DIR_NAME);
    assert!(
        !cards_dir.join("测试/笔记.json").exists(),
        "active 文件应已移走"
    );
    assert!(!cards_dir.join("测试").exists(), "留下的空目录应被清理");
}

#[tokio::test]
/// 笔记从回收站还原后重新打开 Vault：卡片、调度状态与复习历史一起接回。
async fn reattach_restores_cards_and_history_when_note_returns() {
    let (storage, config, vault) = testutil::test_storage().await;
    let note = "测试/笔记.md";
    write_note(vault.path(), note);
    let card_id = save_card(&storage, note).await;
    storage
        .submit_review(SubmitReviewInput {
            card_id: card_id.clone(),
            rating: ReviewRating::Good,
            expected_version: 0,
            idempotency_key: "review-key".to_string(),
        })
        .await
        .expect("提交复习失败");
    // 笔记进系统回收站后卡片被分离。
    std::fs::remove_file(vault.path().join(note)).expect("删除笔记失败");
    detach_note(&storage, note);

    // 用户从回收站还原笔记：同名 .md 重新出现。
    write_note(vault.path(), note);
    let reopened = Storage::open(config.path()).expect("重开存储失败");
    reopened
        .open_vault(vault.path(), &[])
        .await
        .expect("重开 Vault 失败");

    let cards = reopened.list_note_cards(note).await.expect("查询卡片失败");
    assert_eq!(cards.len(), 1);
    assert_eq!(cards[0].id, card_id);
    assert_eq!(cards[0].total_reviews, 1);
    assert_eq!(cards[0].version, 1);
    assert!(
        vault
            .path()
            .join(CARDS_DIR_NAME)
            .join("测试/笔记.json")
            .is_file(),
        "接回后 active 文件应存在"
    );
    assert!(
        !detached_file_path(vault.path(), note).exists(),
        "接回后不应残留隔离文件"
    );
    // 复习历史里的幂等键随文件接回：同键重放走幂等而不是版本冲突。
    let progress = reopened
        .submit_review(SubmitReviewInput {
            card_id: card_id.clone(),
            rating: ReviewRating::Good,
            expected_version: 99,
            idempotency_key: "review-key".to_string(),
        })
        .await
        .expect("幂等重放失败");
    assert_eq!(progress.version, 1);
    assert_eq!(
        reopened
            .get_study_stats()
            .await
            .expect("查询统计失败")
            .weekly_completed_count,
        1
    );
}

#[tokio::test]
/// 笔记仍在回收站时重新打开 Vault 不会接回卡片。
async fn reattach_waits_while_note_is_absent() {
    let (storage, config, vault) = testutil::test_storage().await;
    let note = "测试/笔记.md";
    write_note(vault.path(), note);
    save_card(&storage, note).await;
    std::fs::remove_file(vault.path().join(note)).expect("删除笔记失败");
    detach_note(&storage, note);

    let reopened = Storage::open(config.path()).expect("重开存储失败");
    reopened
        .open_vault(vault.path(), &[])
        .await
        .expect("重开 Vault 失败");

    assert!(reopened
        .list_note_cards(note)
        .await
        .expect("查询卡片失败")
        .is_empty());
    assert!(
        detached_file_path(vault.path(), note).is_file(),
        "笔记未回来时必须保留隔离文件"
    );
    assert!(!vault
        .path()
        .join(CARDS_DIR_NAME)
        .join("测试/笔记.json")
        .exists());
}

#[tokio::test]
/// 隔离区 JSON 不会被当作 active 卡片加载。
async fn detached_files_are_not_loaded_as_active_cards() {
    let (_storage, config, vault) = testutil::test_storage().await;
    let detached = detached_file_path(vault.path(), "幽灵/笔记.md");
    write_detached_file(&detached, Some("幽灵/笔记.md"));

    let reopened = Storage::open(config.path()).expect("重开存储失败");
    reopened
        .open_vault(vault.path(), &[])
        .await
        .expect("重开 Vault 失败");

    assert!(reopened
        .list_note_cards("幽灵/笔记.md")
        .await
        .expect("查询卡片失败")
        .is_empty());
    assert!(reopened
        .get_review_queue(None, true)
        .await
        .expect("查询队列失败")
        .is_empty());
    assert!(detached.is_file(), "未接回的隔离文件应保留");
}

#[tokio::test]
/// 超过保留期的隔离文件被清理，未超期的保留。
async fn purge_removes_only_expired_detached_files() {
    let (_storage, config, vault) = testutil::test_storage().await;
    let base = vault.path().join(CARDS_DIR_NAME).join(DETACHED_DIR_NAME);
    let expired = base.join("旧/笔记.json");
    let fresh = base.join("新/笔记.json");
    for path in [&expired, &fresh] {
        std::fs::create_dir_all(path.parent().expect("隔离文件应有父目录"))
            .expect("创建隔离目录失败");
        std::fs::write(path, "{}").expect("写入隔离文件失败");
    }
    // 把一个隔离文件的 mtime 造到保留期之外。
    let expired_at =
        SystemTime::now() - Duration::from_secs(DETACHED_RETENTION_SECONDS as u64 + 86_400);
    set_modified(&expired, expired_at);

    let reopened = Storage::open(config.path()).expect("重开存储失败");
    reopened
        .open_vault(vault.path(), &[])
        .await
        .expect("重开 Vault 失败");

    assert!(!expired.exists(), "超期隔离文件应被清理");
    assert!(!base.join("旧").exists(), "清理后留下的空目录应被移除");
    assert!(fresh.is_file(), "未超期隔离文件应保留");
}

/// 把文件修改时间改为指定时刻，用于模拟久置文件。
fn set_modified(path: &Path, time: SystemTime) {
    std::fs::File::options()
        .write(true)
        .open(path)
        .expect("打开文件失败")
        .set_modified(time)
        .expect("设置修改时间失败");
}

#[tokio::test]
/// 文件夹分离覆盖前缀下全部笔记，同前缀兄弟目录不受影响。
async fn detach_folder_separates_every_note_under_prefix() {
    let (storage, _config, vault) = testutil::test_storage().await;
    for note in ["课程/一.md", "课程/子/二.md", "课程二/三.md"] {
        write_note(vault.path(), note);
        save_card(&storage, note).await;
    }

    storage
        .inner
        .cards
        .detach_folder_paths("课程")
        .expect("文件夹分离失败");

    assert!(storage
        .list_note_cards("课程/一.md")
        .await
        .expect("查询卡片失败")
        .is_empty());
    assert!(storage
        .list_note_cards("课程/子/二.md")
        .await
        .expect("查询卡片失败")
        .is_empty());
    assert_eq!(
        storage
            .list_note_cards("课程二/三.md")
            .await
            .expect("查询卡片失败")
            .len(),
        1,
        "同前缀兄弟目录不应被分离"
    );
    assert!(detached_file_path(vault.path(), "课程/子/二.md").is_file());
    assert!(vault
        .path()
        .join(CARDS_DIR_NAME)
        .join("课程二/三.json")
        .is_file());
}

#[tokio::test]
/// 分离可重复执行：active 文件已不存在时仍返回成功。
async fn detach_is_idempotent() {
    let (storage, _config, vault) = testutil::test_storage().await;
    let note = "测试/笔记.md";
    write_note(vault.path(), note);
    save_card(&storage, note).await;

    detach_note(&storage, note);
    storage
        .inner
        .cards
        .detach_note_paths(note)
        .expect("重复分离应成功");

    assert!(storage
        .list_note_cards(note)
        .await
        .expect("查询卡片失败")
        .is_empty());
    assert!(detached_file_path(vault.path(), note).is_file());
}

#[tokio::test]
/// 隔离文件缺少 notePath 时按相对路径推导笔记并接回。
async fn reattach_derives_note_path_when_missing() {
    let (_storage, config, vault) = testutil::test_storage().await;
    let note = "推导/笔记.md";
    write_note(vault.path(), note);
    write_detached_file(&detached_file_path(vault.path(), note), None);

    let reopened = Storage::open(config.path()).expect("重开存储失败");
    reopened
        .open_vault(vault.path(), &[])
        .await
        .expect("重开 Vault 失败");

    let cards = reopened.list_note_cards(note).await.expect("查询卡片失败");
    assert_eq!(cards.len(), 1);
    assert_eq!(cards[0].id, "card-1");
}
