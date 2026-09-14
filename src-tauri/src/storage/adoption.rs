//! 草稿批量采纳：先整体校验，在副本中构造，再一次落盘并发布缓存。

use std::collections::HashSet;

use super::{
    card_files::{lock_write, persist_cards, require_root},
    card_records::CardRecord,
    cards::validate_card_input,
    generation_context::{ensure_note_hash, read_context_note},
    now_timestamp, Storage,
};
use crate::{
    card_generation::{card_identity, validate_source},
    error::CommandError,
    models::{AdoptCardsInput, AdoptCardsResult, CardInput, GeneratedCard},
};

impl Storage {
    /// 草稿 UUID 作为最终卡片 ID；重试不覆盖现有卡片内容及复习状态。
    pub async fn adopt_cards(
        &self,
        input: &AdoptCardsInput,
    ) -> Result<AdoptCardsResult, CommandError> {
        if input.cards.is_empty() {
            return Err(CommandError::validation("每次采纳至少需要 1 张卡片"));
        }
        let mut state = lock_write(&self.inner.cards.state)?;
        let root = require_root(&state)?;
        let content = read_context_note(&root, &input.expected_vault_path, &input.note_path)?;
        let mut result = AdoptCardsResult {
            added_ids: Vec::new(),
            existing_ids: Vec::new(),
            duplicate_ids: Vec::new(),
        };
        let mut seen_ids = HashSet::new();
        let mut pending = Vec::new();
        for draft in &input.cards {
            validate_draft_id(&draft.draft_id)?;
            if !seen_ids.insert(draft.draft_id.clone()) {
                return Err(CommandError::validation("同一批草稿包含重复的卡片 ID"));
            }
            if let Some(note) = state.card_note.get(&draft.draft_id) {
                if note != &input.note_path {
                    return Err(CommandError::new(
                        "CARD_ID_CONFLICT",
                        "草稿 ID 已属于其他笔记",
                    ));
                }
                result.existing_ids.push(draft.draft_id.clone());
            } else {
                pending.push(draft_to_card(input, draft, &content)?);
            }
        }
        // 成功响应丢失后的纯重试只确认结果；之后编辑过正文也不会误报未采纳。
        if pending.is_empty() {
            return Ok(result);
        }
        ensure_note_hash(&content, &input.expected_note_hash)?;
        let mut cards = state
            .notes
            .get(&input.note_path)
            .cloned()
            .unwrap_or_default();
        let mut identities: HashSet<String> = cards
            .iter()
            .map(|card| card_identity(&card.kind, &card.front, &card.back))
            .collect();
        let mut position = cards.iter().map(|card| card.position).max().unwrap_or(-1);
        let now = now_timestamp();
        for mut card in pending {
            if !identities.insert(card_identity(&card.kind, &card.front, &card.back)) {
                result.duplicate_ids.push(card.id);
                continue;
            }
            position += 1;
            card.position = position;
            card.created_at = now;
            card.updated_at = now;
            result.added_ids.push(card.id.clone());
            cards.push(card);
        }
        if !result.added_ids.is_empty() {
            // 批次构造期间外部编辑器仍可能改动正文，提交前再核对一次磁盘事实。
            let latest = read_context_note(&root, &input.expected_vault_path, &input.note_path)?;
            ensure_note_hash(&latest, &input.expected_note_hash)?;
            // 持锁直到磁盘提交完成，禁止换 Vault 或并发重试在缓存发布前介入。
            persist_cards(&root, &input.note_path, &cards)?;
            for id in &result.added_ids {
                state.card_note.insert(id.clone(), input.note_path.clone());
            }
            state.notes.insert(input.note_path.clone(), cards);
        }
        Ok(result)
    }
}

/// 限制草稿标识为规范 UUID，避免同一个 UUID 的不同写法绕过幂等索引。
fn validate_draft_id(id: &str) -> Result<(), CommandError> {
    let parsed = uuid::Uuid::parse_str(id)
        .map_err(|_| CommandError::validation("草稿 ID 必须是有效 UUID"))?;
    if parsed.is_nil() || parsed.to_string() != id {
        return Err(CommandError::validation("草稿 ID 必须是规范 UUID"));
    }
    Ok(())
}

/// 把全部可编辑字段一次转换为记录，任何一张无效时整批不落盘。
fn draft_to_card(
    input: &AdoptCardsInput,
    draft: &GeneratedCard,
    content: &str,
) -> Result<CardRecord, CommandError> {
    const FIELDS: &[&str] = &[
        "front", "back", "detail", "example", "aliases", "rubric", "source",
    ];
    for (key, value) in &draft.fields {
        if !FIELDS.contains(&key.as_str()) {
            return Err(CommandError::validation("草稿包含未注册字段"));
        }
        if key != "front" && key != "back" && value.chars().count() > 4_000 {
            return Err(CommandError::validation("卡片补充字段不能超过 4000 个字符"));
        }
    }
    let field = |key: &str| {
        draft
            .fields
            .get(key)
            .map(|value| value.trim().to_string())
            .unwrap_or_default()
    };
    let card_input = CardInput {
        id: Some(draft.draft_id.clone()),
        note_path: input.note_path.clone(),
        source_ref: Some(field("source")),
        source: draft.source.clone(),
        kind: input.kind.clone(),
        front: field("front"),
        back: field("back"),
        detail: Some(field("detail")),
        example: Some(field("example")),
        aliases: split_list_field(&field("aliases")),
        rubric: split_list_field(&field("rubric")),
    };
    validate_card_input(&card_input)?;
    if let Some(source) = &draft.source {
        if !validate_source(content, source)
            || source.excerpt.chars().count() > 4_000
            || source.prefix.chars().count() > 4_000
            || source.suffix.chars().count() > 4_000
            || (!field("source").is_empty() && field("source") != source.excerpt.trim())
        {
            return Err(CommandError::validation("卡片来源与当前笔记不一致"));
        }
    }
    Ok(CardRecord {
        id: draft.draft_id.clone(),
        kind: card_input.kind,
        front: card_input.front,
        back: card_input.back,
        detail: card_input.detail.unwrap_or_default(),
        example: card_input.example.unwrap_or_default(),
        source_ref: card_input
            .source_ref
            .filter(|value| !value.is_empty())
            .or_else(|| draft.source.as_ref().map(|source| source.excerpt.clone()))
            .unwrap_or_default(),
        source: card_input.source,
        aliases: card_input.aliases,
        rubric_points: card_input.rubric,
        ..CardRecord::default()
    })
}

/// 数组 JSON 保持答案中的标点；旧字符串兼容顿号、逗号和换行列表。
fn split_list_field(value: &str) -> Vec<String> {
    let items = serde_json::from_str::<Vec<String>>(value).unwrap_or_else(|_| {
        value
            .split(['、', ',', '，', '\n'])
            .map(String::from)
            .collect()
    });
    items
        .into_iter()
        .map(|item| item.trim().to_string())
        .filter(|item| !item.is_empty())
        .collect()
}

#[cfg(test)]
#[path = "adoption_tests.rs"]
mod tests;
