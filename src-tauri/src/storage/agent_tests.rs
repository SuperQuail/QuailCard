use super::*;
use crate::storage::testutil::{self, TempDir};

#[test]
/// 删除仅移动指定会话，保留可恢复文件并拒绝路径穿越。
fn session_deletion_is_scoped_and_recoverable() {
    let root = TempDir::new();
    let files = AgentFiles::new(root.path()).unwrap();
    let session = files.create_session().unwrap();
    let other = files.create_session().unwrap();
    files.delete_session(&session.id).unwrap();
    assert!(files.session(&session.id).is_err());
    assert!(files.save_session(&session).is_err());
    assert_eq!(files.sessions().unwrap()[0].id, other.id);
    files.delete_session(&session.id).unwrap();
    assert!(files.delete_session("../../note").is_err());
    let archived = std::fs::read_dir(root.path().join(".quailcard/agent/.deleted-sessions"))
        .unwrap()
        .next()
        .unwrap()
        .unwrap()
        .path();
    let saved: AgentSession =
        serde_json::from_str(&std::fs::read_to_string(archived).unwrap()).unwrap();
    assert_eq!(saved.id, session.id);
}

#[test]
/// 协议原始输出只存后端，不随前端会话 DTO 暴露。
fn frontend_session_excludes_protocol_replay() {
    let root = TempDir::new();
    let files = AgentFiles::new(root.path()).unwrap();
    let mut session = files.create_session().unwrap();
    session.messages.push(crate::agent_models::AgentMessage {
        kind: "exchange".into(),
        data: json!({"providerItem":"private"}),
        ..Default::default()
    });
    files.save_session(&session).unwrap();
    assert_eq!(files.session(&session.id).unwrap().messages.len(), 1);
    let public = files.public_session(&session.id).unwrap();
    assert_eq!(public.messages.len(), 1);
    assert_eq!(public.messages[0].kind, "tool_calls");
    assert!(!serde_json::to_string(&public).unwrap().contains("private"));
    assert_eq!(
        files.session(&session.id).unwrap().messages[0].kind,
        "exchange"
    );
}

#[test]
/// 编辑使用真实内容版本，重复请求与撤销均不能覆盖后续人工修改。
fn edits_are_versioned_and_reversible() {
    let root = TempDir::new();
    let files = AgentFiles::new(root.path()).unwrap();
    let created = files
        .change(&Uuid::now_v7().to_string(), "学习.md", "原文", None)
        .unwrap();
    let id = Uuid::now_v7().to_string();
    let edit = files
        .change(&id, "学习.md", "新文", Some(&created.after_hash))
        .unwrap();
    assert_eq!(edit.before.as_deref(), Some("原文"));
    assert_eq!(
        files
            .change(&id, "学习.md", "新文", Some(&created.after_hash))
            .unwrap()
            .id,
        id
    );
    assert!(files
        .change(
            &Uuid::now_v7().to_string(),
            "学习.md",
            "过期写入",
            Some(&created.after_hash)
        )
        .is_err());
    std::fs::write(root.path().join("学习.md"), "人工编辑").unwrap();
    assert!(files.undo(&id, false).is_err());
    assert_eq!(files.read("学习.md").unwrap()["content"], "人工编辑");
    std::fs::write(root.path().join("学习.md"), "新文").unwrap();
    assert_eq!(files.undo(&id, false).unwrap().state, "undone");
    assert_eq!(files.read("学习.md").unwrap()["content"], "原文");
}

#[test]
/// 新笔记撤销保留回收副本且不再被笔记扫描识别。
fn creation_undo_keeps_recovery_copy_and_rejects_cards() {
    let root = TempDir::new();
    let files = AgentFiles::new(root.path()).unwrap();
    let id = Uuid::now_v7().to_string();
    files.change(&id, "new.md", "笔记", None).unwrap();
    assert!(files
        .change(&Uuid::now_v7().to_string(), "new.md", "覆盖", None)
        .is_err());
    assert!(files.undo(&id, true).is_err());
    files.undo(&id, false).unwrap();
    assert!(root
        .path()
        .join(format!(".quailcard/agent/.recycle/{id}.md"))
        .exists());
    assert!(files.vault.scan().unwrap().is_empty());
}

#[test]
/// 路径净化不能被空白、链接别名、内部目录或非 Markdown 后缀绕过。
fn invalid_paths_and_scoped_search_are_rejected() {
    let root = TempDir::new();
    let files = AgentFiles::new(root.path()).unwrap();
    for path in [
        "../secret.md",
        "C:/secret.md",
        ".quailcard/a.md",
        " .quailcard/a.md",
        "a.md ",
        "CON.md",
        "a.md:stream",
        "a.txt",
        "a/../b.md",
        "a\\b.md",
    ] {
        assert!(
            files
                .change(&Uuid::now_v7().to_string(), path, "no", None)
                .is_err(),
            "{path}"
        );
    }
    files
        .change(&Uuid::now_v7().to_string(), "a.md", "共同词语 A", None)
        .unwrap();
    files
        .change(&Uuid::now_v7().to_string(), "b.md", "共同词语 B", None)
        .unwrap();
    let result = files.search("共同", &["a.md".into()]).unwrap();
    assert_eq!(result["notes"].as_array().unwrap().len(), 1);
    assert_eq!(result["notes"][0]["path"], "a.md");
}

#[test]
/// 损坏、高版本和崩溃中的操作不自动重放或重置文件。
fn persistence_rejects_corruption_and_recovers_without_replay() {
    let root = TempDir::new();
    let files = AgentFiles::new(root.path()).unwrap();
    let session = files.create_session().unwrap();
    let path = files.record_path("sessions", &session.id).unwrap();
    std::fs::write(&path, "{broken").unwrap();
    assert!(files.session(&session.id).is_err());
    assert!(files.save_session(&session).is_err());
    assert_eq!(std::fs::read_to_string(&path).unwrap(), "{broken");
    std::fs::write(&path, "{\"formatVersion\":99}").unwrap();
    assert_eq!(
        files.session(&session.id).err().unwrap().code,
        "FILE_FORMAT_NEWER"
    );
    let id = Uuid::now_v7().to_string();
    let mut change = files.change(&id, "a.md", "written", None).unwrap();
    change.state = "pending".into();
    envelope::save_json(&files.record_path("changes", &id).unwrap(), &change).unwrap();
    assert_eq!(files.get_change(&id).unwrap().state, "applied");
    std::fs::remove_file(root.path().join("a.md")).unwrap();
    change.state = "pending".into();
    envelope::save_json(&files.record_path("changes", &id).unwrap(), &change).unwrap();
    assert_eq!(files.get_change(&id).unwrap().state, "notApplied");
    assert!(!root.path().join("a.md").exists());
}

#[tokio::test]
/// 新 Agent 内部文件与名为 agent 的既有笔记目录可共存，损坏会话不阻止卡片加载。
async fn agent_records_do_not_hide_existing_agent_folder_cards() {
    let (storage, config, root) = testutil::test_storage().await;
    let files = AgentFiles::new(root.path()).unwrap();
    let session = files.create_session().unwrap();
    files.save_memory("偏好短讲解").unwrap();
    std::fs::write(
        files.record_path("sessions", &session.id).unwrap(),
        "broken",
    )
    .unwrap();
    let card = storage
        .save_card(crate::models::CardInput {
            id: None,
            note_path: "agent/memory.md".into(),
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
    let reopened = crate::storage::Storage::open(config.path()).unwrap();
    reopened.open_vault(root.path(), &[]).await.unwrap();
    assert_eq!(
        reopened.list_note_cards("agent/memory.md").await.unwrap()[0].id,
        card.id
    );
}
