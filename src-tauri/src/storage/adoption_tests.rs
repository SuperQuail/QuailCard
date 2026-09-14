//! 原子采纳和任务上下文的故障、重试及来源往返验证。

use std::{collections::HashMap, path::Path};

use super::*;
use crate::{
    card_generation::{note_content_hash, resolve_source},
    models::{GenerationContext, GenerationInput, ReviewRating, SubmitReviewInput},
    storage::testutil,
};

const NOTE: &str = "笔记.md";
const BODY: &str = "😀 前文。答案来源。后文\n";

/// 构造稳定 ID 草稿，测试重试时复用同一份输入。
fn draft(front: &str, back: &str) -> GeneratedCard {
    GeneratedCard {
        draft_id: uuid::Uuid::now_v7().to_string(),
        source: None,
        fields: HashMap::from([("front".into(), front.into()), ("back".into(), back.into())]),
    }
}

/// 正文是磁盘事实，输入摘要来自相同已保存快照。
fn input(vault: &Path, cards: Vec<GeneratedCard>) -> AdoptCardsInput {
    std::fs::write(vault.join(NOTE), BODY).unwrap();
    AdoptCardsInput {
        expected_vault_path: vault.to_string_lossy().into_owned(),
        expected_note_hash: note_content_hash(BODY),
        note_path: NOTE.into(),
        kind: "qa".into(),
        cards,
    }
}

/// 与前端命令一致的生成材料快照。
fn generation(vault: &Path) -> GenerationInput {
    GenerationInput {
        type_id: "qa".into(),
        study_mode_id: "self-review".into(),
        note_title: "笔记".into(),
        source_text: BODY.into(),
        images: Vec::new(),
        requested_count: 5,
        context: Some(GenerationContext {
            vault_path: vault.to_string_lossy().into_owned(),
            note_path: NOTE.into(),
            note_hash: note_content_hash(BODY),
            selection: None,
        }),
    }
}

#[tokio::test]
/// 第二张无效时第一张也不能发布到缓存或磁盘。
async fn invalid_second_draft_aborts_entire_batch() {
    let (storage, _config, vault) = testutil::test_storage().await;
    let request = input(
        vault.path(),
        vec![draft("问题", "答案"), draft("", "缺正面")],
    );
    assert!(storage.adopt_cards(&request).await.is_err());
    assert!(storage.list_note_cards(NOTE).await.unwrap().is_empty());
    assert!(!vault.path().join(".quailcard/笔记.json").exists());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
/// 同时双击与重复请求只产生一批卡片，最终 ID 等于草稿 ID。
async fn concurrent_retries_are_idempotent() {
    let (storage, _config, vault) = testutil::test_storage().await;
    let request = input(
        vault.path(),
        vec![draft("问题一", "答案一"), draft("问题二", "答案二")],
    );
    let barrier = std::sync::Arc::new(tokio::sync::Barrier::new(2));
    let submit = |storage: Storage, request: AdoptCardsInput| {
        let barrier = barrier.clone();
        tokio::spawn(async move {
            barrier.wait().await;
            storage.adopt_cards(&request).await
        })
    };
    let (first, second) = tokio::join!(
        submit(storage.clone(), request.clone()),
        submit(storage.clone(), request.clone())
    );
    let (first, second) = (first.unwrap().unwrap(), second.unwrap().unwrap());
    assert_eq!(first.added_ids.len() + second.added_ids.len(), 2);
    assert_eq!(first.existing_ids.len() + second.existing_ids.len(), 2);
    let cards = storage.list_note_cards(NOTE).await.unwrap();
    assert_eq!(cards.len(), 2);
    assert_eq!(cards[0].id, request.cards[0].draft_id);
    assert_eq!(cards[1].id, request.cards[1].draft_id);
}

#[tokio::test]
/// 响应丢失后的重试即使正文已变，也不覆盖成功卡片及调度历史。
async fn successful_retry_preserves_review_and_fields_after_note_edit() {
    let (storage, _config, vault) = testutil::test_storage().await;
    let mut request = input(vault.path(), vec![draft("问题", "答案")]);
    storage.adopt_cards(&request).await.unwrap();
    let progress = storage
        .submit_review(SubmitReviewInput {
            card_id: request.cards[0].draft_id.clone(),
            rating: ReviewRating::Good,
            expected_version: 0,
            idempotency_key: "review-once".into(),
        })
        .await
        .unwrap();
    let original = std::fs::read(vault.path().join(".quailcard/笔记.json")).unwrap();
    request.cards[0]
        .fields
        .insert("back".into(), "不应覆盖".into());
    std::fs::write(vault.path().join(NOTE), "已编辑").unwrap();
    let retry = storage.adopt_cards(&request).await.unwrap();
    assert_eq!(retry.existing_ids, vec![request.cards[0].draft_id.clone()]);
    assert!(retry.added_ids.is_empty());
    let card = &storage.list_note_cards(NOTE).await.unwrap()[0];
    assert_eq!(card.back, "答案");
    assert_eq!(card.version, progress.version);
    assert_eq!(card.total_reviews, 1);
    assert_eq!(
        std::fs::read(vault.path().join(".quailcard/笔记.json")).unwrap(),
        original
    );
}

#[tokio::test]
/// 规范化问题相同或词条相同时跳过，问答卡答案变化不创建重复卡。
async fn identity_deduplicates_existing_and_current_batch() {
    let (storage, _config, vault) = testutil::test_storage().await;
    let request = input(vault.path(), vec![draft("ＨＥＬＬＯ  world", "答案")]);
    storage.adopt_cards(&request).await.unwrap();
    let second = input(
        vault.path(),
        vec![
            draft("hello\tworld", "不同答案"),
            draft("新问题", "甲"),
            draft("新问题", "乙"),
        ],
    );
    let result = storage.adopt_cards(&second).await.unwrap();
    assert_eq!(result.added_ids.len(), 1);
    assert_eq!(result.duplicate_ids.len(), 2);
    let mut vocab = input(
        vault.path(),
        vec![draft("释义一", "ＳＰＥＡＫ"), draft("释义二", "speak")],
    );
    vocab.kind = "vocabulary".into();
    let result = storage.adopt_cards(&vocab).await.unwrap();
    assert_eq!(result.added_ids.len(), 1);
    assert_eq!(result.duplicate_ids.len(), 1);
}

#[tokio::test]
/// aliases、评分要点、音标、例句及 UTF-16 来源均跨重载保留。
async fn adoption_preserves_all_fields_and_sources_on_reload() {
    let (storage, config, vault) = testutil::test_storage().await;
    let mut card = draft("释义", "speak");
    card.source = resolve_source(BODY, "答案来源", None);
    card.fields.extend([
        ("source".into(), "答案来源".into()),
        ("detail".into(), "/spiːk/".into()),
        ("example".into(), "We speak.".into()),
        ("aliases".into(), "词一、词二".into()),
        ("rubric".into(), "[\"事实一，完整\",\"事实二\"]".into()),
    ]);
    let mut request = input(vault.path(), vec![card]);
    request.kind = "vocabulary".into();
    storage.adopt_cards(&request).await.unwrap();
    let reopened = Storage::open(config.path()).unwrap();
    reopened.open_vault(vault.path(), &[]).await.unwrap();
    let cards = reopened.list_note_cards(NOTE).await.unwrap();
    let saved = &cards[0];
    assert_eq!(saved.detail, "/spiːk/");
    assert_eq!(saved.example, "We speak.");
    assert_eq!(saved.aliases, ["词一", "词二"]);
    assert_eq!(saved.rubric_points, ["事实一，完整", "事实二"]);
    assert_eq!(saved.source_ref, "答案来源");
    assert_eq!(saved.source, request.cards[0].source);
    assert_eq!(saved.source.as_ref().unwrap().from, 6);
}

#[tokio::test]
/// 写盘失败时缓存和已有磁盘数据都保持旧值，修复后同批可以安全重试。
async fn write_failure_does_not_publish_partial_cache() {
    let (storage, _config, vault) = testutil::test_storage().await;
    let request = input(vault.path(), vec![draft("问题", "答案")]);
    let target = vault.path().join(".quailcard/笔记.json");
    std::fs::create_dir_all(&target).unwrap();
    assert!(storage.adopt_cards(&request).await.is_err());
    assert!(storage.list_note_cards(NOTE).await.unwrap().is_empty());
    assert!(target.is_dir());
    std::fs::remove_dir(&target).unwrap();
    let result = storage.adopt_cards(&request).await.unwrap();
    assert_eq!(result.added_ids.len(), 1);
}

#[tokio::test]
/// ID 已属于另一篇笔记时整批拒绝，不能移动卡片或覆盖历史。
async fn cross_note_id_conflict_aborts_batch() {
    let (storage, _config, vault) = testutil::test_storage().await;
    let mut request = input(vault.path(), vec![draft("问题", "答案")]);
    storage.adopt_cards(&request).await.unwrap();
    std::fs::write(vault.path().join("另一篇.md"), BODY).unwrap();
    request.note_path = "另一篇.md".into();
    request.cards.insert(0, draft("新问题", "新答案"));
    assert_eq!(
        storage.adopt_cards(&request).await.unwrap_err().code,
        "CARD_ID_CONFLICT"
    );
    assert!(storage
        .list_note_cards("另一篇.md")
        .await
        .unwrap()
        .is_empty());
    assert_eq!(storage.list_note_cards(NOTE).await.unwrap().len(), 1);
}

#[tokio::test]
/// Vault 切换、笔记删除和正文改变分别使旧草稿失效。
async fn changed_vault_deleted_note_and_edited_body_are_rejected() {
    let (storage, _config, vault) = testutil::test_storage().await;
    let request = input(vault.path(), vec![draft("问题", "答案")]);
    std::fs::write(vault.path().join(NOTE), "changed").unwrap();
    assert_eq!(
        storage.adopt_cards(&request).await.unwrap_err().code,
        "GENERATION_NOTE_CHANGED"
    );
    std::fs::remove_file(vault.path().join(NOTE)).unwrap();
    assert_eq!(
        storage.adopt_cards(&request).await.unwrap_err().code,
        "NOTE_NOT_FOUND"
    );
    let other = testutil::TempDir::new();
    storage.open_vault(other.path(), &[]).await.unwrap();
    assert_eq!(
        storage.adopt_cards(&request).await.unwrap_err().code,
        "GENERATION_VAULT_CHANGED"
    );
    assert!(storage.list_note_cards(NOTE).await.unwrap().is_empty());
}

#[tokio::test]
/// 来源越界和路径穿越均在写入之前拒绝。
async fn rejects_invalid_source_and_unsafe_paths() {
    let (storage, _config, vault) = testutil::test_storage().await;
    let mut request = input(vault.path(), vec![draft("问题", "答案")]);
    let mut source = resolve_source(BODY, "答案来源", None).unwrap();
    source.from += 1;
    request.cards[0].source = Some(source);
    assert!(storage.adopt_cards(&request).await.is_err());
    request.cards[0].source = None;
    request.note_path = "../outside.md".into();
    assert!(storage.adopt_cards(&request).await.is_err());
    assert!(storage.list_note_cards(NOTE).await.unwrap().is_empty());
}

#[tokio::test]
/// 生成快照统一换行并保留已保存的完整正文及当前卡片集合。
async fn generation_snapshot_validates_full_document_and_selection() {
    let (storage, _config, vault) = testutil::test_storage().await;
    let request = input(vault.path(), vec![draft("问题", "答案")]);
    storage.adopt_cards(&request).await.unwrap();
    std::fs::write(vault.path().join(NOTE), BODY.replace('\n', "\r\n")).unwrap();
    let mut gen = generation(vault.path());
    let snapshot = storage.validate_generation_context(&gen).unwrap();
    assert_eq!(snapshot.note_content, BODY);
    assert_eq!(snapshot.cards.len(), 1);
    gen.source_text = "答案来源".into();
    gen.context.as_mut().unwrap().selection = resolve_source(BODY, "答案来源", None);
    assert!(storage.validate_generation_context(&gen).is_ok());
    gen.source_text = "不对应选区".into();
    assert!(storage.validate_generation_context(&gen).is_err());
    gen.context.as_mut().unwrap().selection = None;
    assert!(storage.validate_generation_context(&gen).is_err());
}

#[tokio::test]
/// 陈旧摘要、越界选区和文件路径别名不能建立可采纳的拆卡会话。
async fn generation_context_rejects_stale_hash_and_invalid_selection_or_alias() {
    let (storage, _config, vault) = testutil::test_storage().await;
    let _request = input(vault.path(), vec![draft("问题", "答案")]);
    let mut gen = generation(vault.path());
    gen.context.as_mut().unwrap().note_hash = note_content_hash("旧正文");
    assert_eq!(
        storage
            .validate_generation_context(&gen)
            .err()
            .unwrap()
            .code,
        "GENERATION_NOTE_CHANGED"
    );
    gen.context.as_mut().unwrap().note_hash = note_content_hash(BODY);
    let mut source = resolve_source(BODY, "答案来源", None).unwrap();
    source.to += 100;
    gen.source_text = "答案来源".into();
    gen.context.as_mut().unwrap().selection = Some(source);
    assert_eq!(
        storage
            .validate_generation_context(&gen)
            .err()
            .unwrap()
            .code,
        "GENERATION_SOURCE_CHANGED"
    );
    gen = generation(vault.path());
    gen.context.as_mut().unwrap().note_path = "笔记.md/".into();
    assert!(storage.validate_generation_context(&gen).is_err());
    gen.context.as_mut().unwrap().note_path = "./笔记.md".into();
    assert!(storage.validate_generation_context(&gen).is_err());
    gen.context.as_mut().unwrap().note_path = "../笔记.md".into();
    assert!(storage.validate_generation_context(&gen).is_err());
}

#[tokio::test]
/// 不带笔记上下文的旧接口保持可用，只返回传入材料且不绑定 Vault 卡片。
async fn legacy_generation_snapshot_uses_only_supplied_material() {
    let (storage, _config, vault) = testutil::test_storage().await;
    let mut gen = generation(vault.path());
    gen.context = None;
    gen.source_text = "粘贴内容\r\n下一行".into();
    let snapshot = storage.validate_generation_context(&gen).unwrap();
    assert_eq!(snapshot.note_content, "粘贴内容\n下一行");
    assert!(snapshot.cards.is_empty());
}
