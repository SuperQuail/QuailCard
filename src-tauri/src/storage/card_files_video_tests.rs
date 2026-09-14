//! 视频内部 JSON 不属于卡片；重开知识库不能因空 notePath 或任务损坏而失败。
use super::*;
use crate::storage::{
    testutil,
    video::{VideoStorage, VideoTaskRecord},
};

#[tokio::test]
/// 复现用户的 null notePath 任务，同时保留 video 目录中真实卡片。
async fn reopens_vault_with_video_tasks_and_existing_cards() {
    let (storage, config, root) = testutil::test_storage().await;
    let video = VideoStorage::new(root.path());
    let id = uuid::Uuid::now_v7().to_string();
    let record = VideoTaskRecord::new(&id, "bilibili:BV1:p1", "https://www.bilibili.com/video/BV1");
    video.save_task(&record).unwrap();
    let task_file = video.task_dir(&id).unwrap().join("task.json");
    let original = std::fs::read(&task_file).unwrap();
    let mut cards = Vec::new();
    for note in [
        "video/memory.md",
        "video/tasks/memory.md",
        "video/pages/memory.md",
        "video/pages/BV1-123.md",
    ] {
        let card = storage
            .save_card(crate::models::CardInput {
                id: None,
                note_path: note.into(),
                source_ref: None,
                source: None,
                kind: "qa".into(),
                front: "Q".into(),
                back: "A".into(),
                detail: None,
                example: None,
                aliases: vec![],
                rubric: vec![],
            })
            .await
            .unwrap();
        cards.push((note, card.id));
    }
    // 转录缓存的版本或损坏不应参与卡片文件的版本检查。
    let pages = root.path().join(".quailcard/video/pages");
    std::fs::create_dir_all(&pages).unwrap();
    std::fs::write(
        pages.join("BV2-456.json"),
        r#"{"formatVersion":99,"segments":[]}"#,
    )
    .unwrap();
    std::fs::write(
        video.task_dir(&id).unwrap().join("transcript.json"),
        "broken",
    )
    .unwrap();
    let reopened = crate::storage::Storage::open(config.path()).unwrap();
    reopened.open_vault(root.path(), &[]).await.unwrap();
    for (note, id) in cards {
        assert_eq!(reopened.list_note_cards(note).await.unwrap()[0].id, id);
    }
    assert_eq!(std::fs::read(&task_file).unwrap(), original);
}

#[tokio::test]
/// 真正卡片文件的 null notePath 仍然拒绝加载，不能用宽松解析掩盖数据损坏。
async fn still_rejects_corrupt_card_files() {
    let (_storage, config, root) = testutil::test_storage().await;
    let path = root.path().join(".quailcard/video/memory.json");
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    let raw = r#"{"formatVersion":1,"notePath":null,"cards":[]}"#;
    std::fs::write(&path, raw).unwrap();
    let reopened = crate::storage::Storage::open(config.path()).unwrap();
    assert_eq!(
        reopened
            .open_vault(root.path(), &[])
            .await
            .unwrap_err()
            .code,
        "CARD_FILE_CORRUPT"
    );
    assert_eq!(std::fs::read_to_string(path).unwrap(), raw);
}

#[test]
/// 保留路径只覆盖 UUID 任务和 CID 缓存；同名真实笔记优先受卡片损坏保护。
fn only_excludes_internal_layout() {
    let root = testutil::TempDir::new();
    let relative = "video/tasks/01a09123-61cd-77e3-970c-b0b32a0fa9c2/task.json";
    let path = root.path().join(".quailcard").join(relative);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(&path, "broken video task").unwrap();
    assert!(is_video_internal_file(root.path(), &path, relative));
    let note = root.path().join(derive_note_path(relative));
    std::fs::create_dir_all(note.parent().unwrap()).unwrap();
    std::fs::write(note, "real note").unwrap();
    assert!(!is_video_internal_file(root.path(), &path, relative));
    assert!(!is_video_internal_file(
        root.path(),
        &path,
        "video/tasks/my-note.json"
    ));
    assert!(!is_video_internal_file(
        root.path(),
        &path,
        "video/pages/my-note.json"
    ));
}
