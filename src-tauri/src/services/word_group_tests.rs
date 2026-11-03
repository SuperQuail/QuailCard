//! 单词组用例测试：创建、非法内容不落盘、保存后索引与卡片一致。

use super::{create, save};
use crate::{
    storage::testutil,
    vaultfs::VaultState,
    word_group::{self, WordGroupDocument, WordPage, FORMAT_VERSION},
};

/// 构造已选择测试 Vault 目录的 VaultState。
fn vault_state(vault: &testutil::TempDir) -> VaultState {
    let state = VaultState::new();
    state.set_root(vault.path().to_path_buf()).unwrap();
    state
}

/// 构造一个完整页。
fn page(id: &str, word: &str, meaning: &str) -> WordPage {
    WordPage {
        id: id.to_string(),
        word: word.to_string(),
        meaning: meaning.to_string(),
        ..WordPage::default()
    }
}

/// 构造合法文档并序列化为磁盘文本。
fn serialized(words: Vec<WordPage>) -> String {
    word_group::serialize(&WordGroupDocument {
        format_version: FORMAT_VERSION,
        words,
    })
    .unwrap()
}

#[tokio::test]
/// 新建单词组返回 NoteFile：路径带 .qword、标题去扩展名、只有 1 个空草稿页。
async fn creates_word_group_file_with_single_draft_page() {
    let (storage, _config, vault) = testutil::test_storage().await;
    let state = vault_state(&vault);
    let file = create(&state, &storage, "课程", "单词").await.unwrap();
    assert_eq!(file.path, "课程/单词.qword");
    assert_eq!(file.title, "单词");
    assert!(vault.path().join("课程/单词.qword").is_file());
    let document = word_group::parse(&file.content).unwrap();
    assert_eq!(document.words.len(), 1);
    assert!(document.words[0].word.is_empty());
    assert!(storage
        .list_note_cards(&file.path)
        .await
        .unwrap()
        .is_empty());
    let notes = storage.list_notes().await.unwrap();
    let summary = notes
        .iter()
        .find(|note| note.path == file.path)
        .expect("新建的单词组应出现在笔记列表");
    assert_eq!(summary.title, "单词");
    assert_eq!(summary.tags_json, "[]");
}

#[tokio::test]
/// 非法 JSON / 高版本 / 结构错误都在写盘前拒绝，不会产生半截文件。
async fn rejects_invalid_content_without_writing() {
    let (storage, _config, vault) = testutil::test_storage().await;
    let state = vault_state(&vault);
    let path = "组.qword";
    for (content, expected) in [
        ("{不是 JSON", "WORD_GROUP_INVALID"),
        (
            r#"{"formatVersion":9,"words":[]}"#,
            "WORD_GROUP_FORMAT_NEWER",
        ),
        (
            r#"{"formatVersion":1,"words":[{"id":"带 空格"}]}"#,
            "WORD_GROUP_INVALID",
        ),
    ] {
        let error = save(&state, &storage, path, content).await.unwrap_err();
        assert_eq!(error.code, expected);
        assert!(!vault.path().join(path).exists(), "非法内容不得落盘");
    }
}

#[tokio::test]
/// 保存成功：内容写盘、索引可搜索、完整页物化为 vocabulary 卡片。
async fn save_writes_content_indexes_and_materializes_cards() {
    let (storage, _config, vault) = testutil::test_storage().await;
    let state = vault_state(&vault);
    let file = create(&state, &storage, "", "单词").await.unwrap();
    let content = serialized(vec![page("p1", "ephemeral", "短暂的")]);
    let mtime = save(&state, &storage, &file.path, &content).await.unwrap();
    assert!(mtime > 0);
    assert_eq!(
        std::fs::read_to_string(vault.path().join(&file.path)).unwrap(),
        content,
        "保存后磁盘内容与提交内容一致"
    );

    let cards = storage.list_note_cards(&file.path).await.unwrap();
    assert_eq!(cards.len(), 1);
    assert_eq!(cards[0].front, "短暂的");
    assert_eq!(cards[0].back, "ephemeral");

    let result = storage.search("短暂的").await.unwrap();
    assert!(
        result.notes.iter().any(|hit| hit.path == file.path),
        "搜索应命中单词组释义"
    );
}

#[tokio::test]
/// 非 .qword 路径不得走单词组保存流程。
async fn rejects_non_word_group_path() {
    let (storage, _config, vault) = testutil::test_storage().await;
    let state = vault_state(&vault);
    let error = save(&state, &storage, "笔记.md", "{}").await.unwrap_err();
    assert_eq!(error.code, "VALIDATION_ERROR");
}
