//! .quailcard 卡片内存缓存与用例：加载、增删改与统计。
//!
//! 每篇笔记在 .quailcard/ 下有一个镜像 JSON 文件（卡片 + 调度 + 历史），
//! 打开 Vault 时全量加载进内存，之后所有读写先改内存再原子写透磁盘
//! （磁盘布局与 IO 细节见 card_files 模块）。Vault 未打开时读操作返回
//! 空结果、写操作报错，与旧全局库行为对齐。

use std::{
    collections::{BTreeMap, HashMap},
    path::Path,
    sync::RwLock,
};

use super::{
    card_files::{
        card_relative_path, cleanup_empty_parents, load_card_files, lock_write, persist_cards,
        remove_card_from_note, remove_note_cards, require_root,
    },
    card_records::CardRecord,
    now_timestamp, Storage,
};
use crate::{
    error::CommandError,
    models::{AdoptCardsInput, CardInput, NoteCard},
};

/// 卡片内存缓存：BTreeMap 保证按笔记路径稳定遍历。
#[derive(Default)]
pub(crate) struct CardStore {
    pub(super) state: RwLock<CardState>,
}

/// 卡片缓存的全部可变状态；只在同步代码段内持锁。
#[derive(Default)]
pub(super) struct CardState {
    /// 当前 Vault 根目录，未打开时为 None。
    pub(super) root: Option<std::path::PathBuf>,
    /// note_path -> 卡片列表（按 position 升序）。
    pub(super) notes: BTreeMap<String, Vec<CardRecord>>,
    /// card_id -> note_path 反向索引。
    pub(super) card_note: HashMap<String, String>,
    /// idempotency_key -> card_id，保证复习提交幂等。
    pub(super) idempotency: HashMap<String, String>,
}

impl CardStore {
    /// 设置 Vault 根目录并全量加载 .quailcard 卡片文件。
    ///
    /// 重新扫描（外部修改检测）也走本方法：以磁盘为准整体替换内存。
    pub(crate) fn set_root(&self, root: &Path) -> Result<(), CommandError> {
        let mut fresh = CardState {
            root: Some(root.to_path_buf()),
            ..CardState::default()
        };
        load_card_files(root, &mut fresh)?;
        *lock_write(&self.state)? = fresh;
        Ok(())
    }

    /// 保存或更新单张卡片并写透磁盘，返回面板模型。
    pub(crate) fn save_card(&self, input: CardInput) -> Result<NoteCard, CommandError> {
        validate_card_input(&input)?;
        let card_id = input
            .id
            .clone()
            .filter(|id| !id.trim().is_empty() && !id.starts_with("draft-"))
            .unwrap_or_else(|| uuid::Uuid::now_v7().to_string());
        let now = now_timestamp();
        let mut state = lock_write(&self.state)?;
        let root = require_root(&state)?;
        // 卡片换笔记属于异常路径：先从旧笔记移除再落入目标笔记。
        if let Some(old_note) = state.card_note.get(&card_id).cloned() {
            if old_note != input.note_path {
                remove_card_from_note(&mut state, &old_note, &card_id, &root)?;
            }
        }
        let note_card = {
            let cards = state.notes.entry(input.note_path.clone()).or_default();
            let existing = cards.iter().position(|card| card.id == card_id);
            let record = CardRecord {
                id: card_id.clone(),
                kind: input.kind.clone(),
                front: input.front.trim().to_string(),
                back: input.back.trim().to_string(),
                detail: input.detail.clone().unwrap_or_default(),
                example: input.example.clone().unwrap_or_default(),
                source_ref: input.source_ref.clone().unwrap_or_default(),
                aliases: input.aliases.clone(),
                rubric_points: input.rubric.clone(),
                position: existing
                    .map(|index| cards[index].position)
                    .unwrap_or_else(|| {
                        cards.iter().map(|card| card.position).max().unwrap_or(-1) + 1
                    }),
                created_at: existing.map(|index| cards[index].created_at).unwrap_or(now),
                updated_at: now,
                review: existing
                    .map(|index| cards[index].review.clone())
                    .unwrap_or_default(),
                history: existing
                    .map(|index| cards[index].history.clone())
                    .unwrap_or_default(),
            };
            let note_card = record.to_note_card(&input.note_path);
            match existing {
                Some(index) => cards[index] = record,
                None => cards.push(record),
            }
            cards.sort_by_key(|card| card.position);
            note_card
        };
        state.card_note.insert(card_id, input.note_path.clone());
        let cards = state
            .notes
            .get(&input.note_path)
            .cloned()
            .unwrap_or_default();
        persist_cards(&root, &input.note_path, &cards)?;
        Ok(note_card)
    }

    /// 删除卡片：磁盘文件随卡片清空一并移除。
    pub(crate) fn delete_card(&self, card_id: &str) -> Result<(), CommandError> {
        let mut state = lock_write(&self.state)?;
        let root = require_root(&state)?;
        let Some(note_path) = state.card_note.remove(card_id) else {
            return Err(CommandError::new("CARD_NOT_FOUND", "卡片不存在"));
        };
        remove_card_from_note(&mut state, &note_path, card_id, &root)?;
        state.idempotency.retain(|_, id| id != card_id);
        Ok(())
    }

    /// 批量采纳 AI 拆卡草稿，返回成功写入数量。
    pub(crate) fn adopt_cards(&self, input: &AdoptCardsInput) -> Result<usize, CommandError> {
        let mut count = 0;
        for draft in &input.cards {
            let field = |key: &str| -> String {
                draft
                    .fields
                    .get(key)
                    .cloned()
                    .unwrap_or_default()
                    .trim()
                    .to_string()
            };
            let front = field("front");
            let back = field("back");
            if front.is_empty() || back.is_empty() {
                continue;
            }
            let card = CardInput {
                id: None,
                note_path: input.note_path.clone(),
                source_ref: Some(field("source")),
                kind: input.kind.clone(),
                front,
                back,
                detail: Some(field("detail")),
                example: Some(field("example")),
                aliases: split_list_field(&field("aliases")),
                rubric: split_list_field(&field("rubric")),
            };
            self.save_card(card)?;
            count += 1;
        }
        Ok(count)
    }

    /// 重命名笔记/文件夹后同步移动镜像卡片文件并更新索引。
    pub(crate) fn rename_note_paths(
        &self,
        old_prefix: &str,
        new_prefix: &str,
    ) -> Result<(), CommandError> {
        let mut state = lock_write(&self.state)?;
        let root = require_root(&state)?;
        let child_prefix = format!("{old_prefix}/");
        let affected: Vec<String> = state
            .notes
            .keys()
            .filter(|key| key.as_str() == old_prefix || key.starts_with(&child_prefix))
            .cloned()
            .collect();
        for key in affected {
            let new_key = if key == old_prefix {
                new_prefix.to_string()
            } else {
                format!("{new_prefix}/{}", &key[old_prefix.len() + 1..])
            };
            if let Some(cards) = state.notes.remove(&key) {
                state.notes.insert(new_key.clone(), cards);
            }
            let source = root.join(card_relative_path(&key)?);
            let target = root.join(card_relative_path(&new_key)?);
            if source.exists() {
                if let Some(parent) = target.parent() {
                    std::fs::create_dir_all(parent)?;
                }
                std::fs::rename(&source, &target)?;
                cleanup_empty_parents(&root, source.parent());
            }
            for note in state.card_note.values_mut() {
                if *note == key {
                    *note = new_key.clone();
                }
            }
        }
        Ok(())
    }

    /// 删除单篇笔记的全部卡片及镜像文件。
    pub(crate) fn delete_note_paths(&self, note_path: &str) -> Result<(), CommandError> {
        let mut state = lock_write(&self.state)?;
        let root = require_root(&state)?;
        remove_note_cards(&mut state, note_path, &root)
    }

    /// 删除文件夹前缀下全部笔记的卡片及镜像文件。
    pub(crate) fn delete_folder_paths(&self, prefix: &str) -> Result<(), CommandError> {
        let mut state = lock_write(&self.state)?;
        let root = require_root(&state)?;
        let child_prefix = format!("{prefix}/");
        let affected: Vec<String> = state
            .notes
            .keys()
            .filter(|key| key.as_str() == prefix || key.starts_with(&child_prefix))
            .cloned()
            .collect();
        for key in affected {
            remove_note_cards(&mut state, &key, &root)?;
        }
        Ok(())
    }

    /// 统计每篇笔记的卡片数与到期数（供笔记列表使用）。
    pub(super) fn note_counts(&self, now: i64) -> HashMap<String, (i64, i64)> {
        let Ok(state) = self.state.read() else {
            return HashMap::new();
        };
        state
            .notes
            .iter()
            .map(|(note_path, cards)| {
                let due = cards
                    .iter()
                    .filter(|card| card.review.due_at <= now)
                    .count() as i64;
                (note_path.clone(), (cards.len() as i64, due))
            })
            .collect()
    }

    /// 克隆全部卡片快照（复习队列、搜索与统计的读取基础）。
    pub(super) fn snapshot_cards(&self) -> Vec<(String, CardRecord)> {
        let Ok(state) = self.state.read() else {
            return Vec::new();
        };
        state
            .notes
            .iter()
            .flat_map(|(note_path, cards)| {
                cards
                    .iter()
                    .map(|card| (note_path.clone(), card.clone()))
                    .collect::<Vec<_>>()
            })
            .collect()
    }

    /// 读取指定笔记的全部卡片。
    pub(super) fn cards_of(&self, note_path: &str) -> Vec<CardRecord> {
        let Ok(state) = self.state.read() else {
            return Vec::new();
        };
        state.notes.get(note_path).cloned().unwrap_or_default()
    }

    /// 返回当前 Vault 的卡片数据目录；未打开 Vault 时为 None。
    pub(super) fn cards_dir(&self) -> Option<std::path::PathBuf> {
        self.state
            .read()
            .ok()
            .and_then(|state| state.root.clone())
            .map(|root| root.join(super::card_files::CARDS_DIR_NAME))
    }
}

impl Storage {
    /// 查询指定笔记的全部卡片及调度状态。
    pub async fn list_note_cards(&self, note_path: &str) -> Result<Vec<NoteCard>, CommandError> {
        Ok(self
            .inner
            .cards
            .cards_of(note_path)
            .iter()
            .map(|card| card.to_note_card(note_path))
            .collect())
    }

    /// 保存或更新单张卡片。
    pub async fn save_card(&self, input: CardInput) -> Result<NoteCard, CommandError> {
        self.inner.cards.save_card(input)
    }

    /// 删除单张卡片。
    pub async fn delete_card(&self, card_id: &str) -> Result<(), CommandError> {
        self.inner.cards.delete_card(card_id)
    }

    /// 采纳 AI 拆卡草稿并批量写入卡片。
    pub async fn adopt_cards(&self, input: &AdoptCardsInput) -> Result<usize, CommandError> {
        self.inner.cards.adopt_cards(input)
    }
}

/// 校验单张卡片的通用约束。
fn validate_card_input(input: &CardInput) -> Result<(), CommandError> {
    if !matches!(input.kind.as_str(), "vocabulary" | "qa") {
        return Err(CommandError::validation("卡片类型无效"));
    }
    if input.note_path.trim().is_empty() || input.note_path.chars().count() > 512 {
        return Err(CommandError::validation("笔记路径无效"));
    }
    let front = input.front.trim();
    let back = input.back.trim();
    if front.is_empty() || front.chars().count() > 2000 {
        return Err(CommandError::validation("卡片正面长度必须为 1-2000 个字符"));
    }
    if back.is_empty() || back.chars().count() > 8000 {
        return Err(CommandError::validation("卡片背面长度必须为 1-8000 个字符"));
    }
    Ok(())
}

/// 将逗号或顿号分隔的字段拆成列表。
fn split_list_field(value: &str) -> Vec<String> {
    value
        .split(['、', ',', '，'])
        .map(str::trim)
        .filter(|item| !item.is_empty())
        .map(String::from)
        .collect()
}

#[cfg(test)]
#[path = "cards_tests.rs"]
mod tests;
