//! 单词组卡片物化：把 .qword 文档同步为可复习的 vocabulary 卡片。
//!
//! 单词组是事实来源，卡片是派生数据：每次保存后按页面顺序重建该笔记的
//! 卡片集合，保留已有卡片的复习状态、历史与创建时间；只有实际变化时才
//! 写盘。页面 id 属于其它笔记时跳过，绝不抢占别人的卡片。

use std::{collections::HashSet, path::Path};

use super::{
    card_files::{lock_write, persist_cards, require_root},
    card_records::CardRecord,
    cards::CardState,
    now_timestamp, Storage,
};
use crate::{
    error::CommandError,
    vaultfs::documents::DocumentKind,
    word_group::{self, WordCardFields, WordGroupDocument},
};

impl Storage {
    /// 把一份已校验的单词组文档物化为卡片；返回卡片集合是否发生变化。
    ///
    /// 调用方（保存用例）必须已完成 parse 校验；这里再持写锁，先写盘成功
    /// 再发布内存缓存，失败时缓存保持旧版本，脏稿可安全重试。
    pub(crate) fn sync_word_group_cards(
        &self,
        note_path: &str,
        document: &WordGroupDocument,
    ) -> Result<bool, CommandError> {
        let mut state = lock_write(&self.inner.cards.state)?;
        let root = require_root(&state)?;
        materialize(&mut state, &root, note_path, document)
    }

    /// 打开/重扫 Vault 时对账全部单词组卡片。
    ///
    /// 契约：解析失败的单词组只记日志跳过（文件仍出现在列表里，编辑器
    /// 只读提示）；单组卡片同步失败也不阻断 Vault 打开，避免一个坏文件
    /// 让整个知识库打不开。无变化时不写盘。
    pub(crate) fn reconcile_word_group_cards(&self, files: &[(String, String, i64)]) {
        if !files
            .iter()
            .any(|(path, ..)| DocumentKind::of_path(path) == Some(DocumentKind::WordGroup))
        {
            return;
        }
        let mut state = match lock_write(&self.inner.cards.state) {
            Ok(state) => state,
            Err(error) => {
                eprintln!("WORD_GROUP_RECONCILE_LOCK_ERROR(detail): {error}");
                return;
            }
        };
        let root = match require_root(&state) {
            Ok(root) => root,
            Err(_) => return,
        };
        for (path, content, _) in files {
            if DocumentKind::of_path(path) != Some(DocumentKind::WordGroup) {
                continue;
            }
            let document = match word_group::parse(content) {
                Ok(document) => document,
                Err(error) => {
                    eprintln!("WORD_GROUP_RECONCILE_SKIP(detail): {error} path={path}");
                    continue;
                }
            };
            if let Err(error) = materialize(&mut state, &root, path, &document) {
                eprintln!("WORD_GROUP_RECONCILE_ERROR(detail): {error} path={path}");
            }
        }
    }
}

/// 物化核心：按页面顺序重建卡片的期望集合，仅在内容变化时落盘并发布缓存。
///
/// 规则（与方案 3.3 一致）：
/// - 完整页（word+meaning）新建或更新同 id 卡片，保留 review/history/createdAt；
/// - 草稿页不新建卡片，已有同 id 卡片原样保留（只跟随页码，避免页面删除后
///   位置错乱），防止编辑中间态删掉复习历史；
/// - 页面 id 消失才删卡（连同历史与幂等键）；
/// - id 属于其它笔记时跳过并记日志，不抢占。
fn materialize(
    state: &mut CardState,
    root: &Path,
    note_path: &str,
    document: &WordGroupDocument,
) -> Result<bool, CommandError> {
    let existing = state.notes.get(note_path).cloned().unwrap_or_default();
    let now = now_timestamp();
    let mut next: Vec<CardRecord> = Vec::with_capacity(document.words.len());
    for (index, page) in document.words.iter().enumerate() {
        let position = index as i64;
        let current = existing.iter().find(|card| card.id == page.id);
        if current.is_none() && page_id_owned_elsewhere(state, &page.id, note_path) {
            eprintln!(
                "WORD_GROUP_CARD_SKIP(detail): id 已属于其他笔记 id={} note={note_path}",
                page.id
            );
            continue;
        }
        match word_group::card_fields(page) {
            Some(fields) => next.push(match current {
                Some(card) => update_complete_card(card, &fields, position, now),
                None => new_complete_card(&page.id, &fields, position, now),
            }),
            None => {
                if let Some(card) = current {
                    let mut kept = card.clone();
                    kept.position = position;
                    next.push(kept);
                }
            }
        }
    }
    if next == existing {
        return Ok(false);
    }
    // 先落盘再发布缓存：写入失败时内存仍是旧版本，调用方可重试。
    persist_cards(root, note_path, &next)?;
    publish_cards(state, note_path, &existing, &next);
    Ok(true)
}

/// 判断页面 id 是否已被其它笔记的卡片占用（跨笔记 id 必须跳过）。
fn page_id_owned_elsewhere(state: &CardState, card_id: &str, note_path: &str) -> bool {
    state
        .card_note
        .get(card_id)
        .is_some_and(|owner| owner != note_path)
}

/// 用页面字段刷新完整页卡片：保留复习状态、历史与创建时间。
///
/// 只有内容字段真的变了才更新 updated_at；页码平移属于派生信息，不制造
/// "内容已更新"的假象。
fn update_complete_card(
    card: &CardRecord,
    fields: &WordCardFields,
    position: i64,
    now: i64,
) -> CardRecord {
    let content_changed = card.kind != "vocabulary"
        || card.front != fields.front
        || card.back != fields.back
        || card.detail != fields.detail
        || card.example != fields.example
        || !card.source_ref.is_empty()
        || card.source.is_some()
        || !card.aliases.is_empty()
        || !card.rubric_points.is_empty();
    CardRecord {
        id: card.id.clone(),
        kind: "vocabulary".to_string(),
        front: fields.front.clone(),
        back: fields.back.clone(),
        detail: fields.detail.clone(),
        example: fields.example.clone(),
        source_ref: String::new(),
        source: None,
        aliases: Vec::new(),
        rubric_points: Vec::new(),
        position,
        created_at: card.created_at,
        updated_at: if content_changed {
            now
        } else {
            card.updated_at
        },
        review: card.review.clone(),
        history: card.history.clone(),
    }
}

/// 新建完整页卡片：默认调度状态（立即到期），来源与别名按契约留空。
fn new_complete_card(
    card_id: &str,
    fields: &WordCardFields,
    position: i64,
    now: i64,
) -> CardRecord {
    CardRecord {
        id: card_id.to_string(),
        kind: "vocabulary".to_string(),
        front: fields.front.clone(),
        back: fields.back.clone(),
        detail: fields.detail.clone(),
        example: fields.example.clone(),
        position,
        created_at: now,
        updated_at: now,
        ..CardRecord::default()
    }
}

/// 卡片文件写盘成功后发布内存缓存：补齐新 id 映射、移除消失卡片的历史索引。
fn publish_cards(
    state: &mut CardState,
    note_path: &str,
    previous: &[CardRecord],
    next: &[CardRecord],
) {
    let ids: HashSet<&str> = next.iter().map(|card| card.id.as_str()).collect();
    for card in previous {
        if !ids.contains(card.id.as_str()) {
            state.card_note.remove(&card.id);
            state.idempotency.retain(|_, card_id| card_id != &card.id);
        }
    }
    for card in next {
        state
            .card_note
            .insert(card.id.clone(), note_path.to_string());
    }
    if next.is_empty() {
        state.notes.remove(note_path);
    } else {
        state.notes.insert(note_path.to_string(), next.to_vec());
    }
}

#[cfg(test)]
#[path = "word_groups_tests.rs"]
mod tests;
