//! 内存检索：复习队列、听写判定与全文搜索。
//!
//! 文件方案去除了 FTS5 倒排索引，改为对内存快照做大小写不敏感的
//! 子串匹配（多词 AND）；本地笔记量级下扫描耗时可忽略。

use super::{helpers, now_timestamp, Storage};
use crate::{
    error::CommandError,
    models::{CardHit, DictationResult, NoteHit, ReviewCard, SearchResult},
};

/// 搜索结果每类命中的最大数量。
const MAX_HITS: usize = 20;

impl Storage {
    /// 读取复习队列：可限定笔记；include_all 为真时包含未到期卡片。
    pub async fn get_review_queue(
        &self,
        note_path: Option<&str>,
        include_all: bool,
    ) -> Result<Vec<ReviewCard>, CommandError> {
        let now = now_timestamp();
        let mut matched = self
            .inner
            .cards
            .snapshot_cards()
            .into_iter()
            .filter(|(card_note, card)| {
                note_path.is_none_or(|filter| card_note == filter)
                    && (include_all || card.review.due_at <= now)
            })
            .collect::<Vec<_>>();
        // 与旧 SQL 排序一致：重学优先，其次到期时间、笔记路径与位置。
        matched.sort_by(|(left_note, left), (right_note, right)| {
            phase_rank(&left.review.scheduler_phase)
                .cmp(&phase_rank(&right.review.scheduler_phase))
                .then_with(|| left.review.due_at.cmp(&right.review.due_at))
                .then_with(|| left_note.cmp(right_note))
                .then_with(|| left.position.cmp(&right.position))
        });
        Ok(matched
            .into_iter()
            .map(|(card_note, card)| ReviewCard {
                id: card.id,
                note_path: card_note,
                source_ref: card.source_ref,
                kind: card.kind,
                front: card.front,
                back: card.back,
                detail: card.detail,
                example: card.example,
                aliases: card.aliases,
                rubric_points: card.rubric_points,
                state: card.review.scheduler_phase,
                version: card.review.version,
            })
            .collect())
    }

    /// 后端权威听写判定：规范化后与单词及别名比对。
    pub async fn check_dictation(
        &self,
        card_id: &str,
        answer: &str,
    ) -> Result<DictationResult, CommandError> {
        let state = self
            .inner
            .cards
            .state
            .read()
            .map_err(|_| CommandError::new("INTERNAL_ERROR", "卡片存储状态锁失效"))?;
        let note_path = state
            .card_note
            .get(card_id)
            .ok_or_else(|| CommandError::new("CARD_NOT_FOUND", "单词卡不存在"))?;
        let card = state
            .notes
            .get(note_path)
            .and_then(|cards| cards.iter().find(|card| card.id == card_id))
            .ok_or_else(|| CommandError::new("CARD_NOT_FOUND", "单词卡不存在"))?;
        if card.kind != "vocabulary" {
            return Err(CommandError::new("CARD_NOT_FOUND", "单词卡不存在"));
        }
        let expected = card.back.clone();
        let aliases = card.aliases.clone();
        let normalized = helpers::normalize_answer(answer);
        let correct = !normalized.is_empty()
            && std::iter::once(expected.as_str())
                .chain(aliases.iter().map(String::as_str))
                .map(helpers::normalize_answer)
                .any(|value| value == normalized);
        Ok(DictationResult {
            correct,
            expected,
            aliases,
        })
    }

    /// 同时搜索笔记正文与卡片正反面，返回高亮片段。
    pub async fn search(&self, query: &str) -> Result<SearchResult, CommandError> {
        let keyword = query.trim();
        if keyword.is_empty() {
            return Ok(SearchResult {
                notes: Vec::new(),
                cards: Vec::new(),
            });
        }
        let tokens: Vec<String> = keyword.split_whitespace().map(str::to_lowercase).collect();
        let mut notes = Vec::new();
        for entry in self.inner.notes.snapshot() {
            if notes.len() >= MAX_HITS {
                break;
            }
            let haystack = format!("{} {}", entry.title, entry.content).to_lowercase();
            if tokens.iter().all(|token| haystack.contains(token)) {
                notes.push(NoteHit {
                    path: entry.path.clone(),
                    title: entry.title.clone(),
                    snippet: helpers::build_snippet(&entry.content, &tokens[0]),
                });
            }
        }
        let mut cards = Vec::new();
        for (note_path, card) in self.inner.cards.snapshot_cards() {
            if cards.len() >= MAX_HITS {
                break;
            }
            let haystack = format!("{} {}", card.front, card.back).to_lowercase();
            if tokens.iter().all(|token| haystack.contains(token)) {
                cards.push(CardHit {
                    card_id: card.id.clone(),
                    note_path,
                    front: card.front.clone(),
                    snippet: helpers::build_snippet(&haystack, &tokens[0]),
                });
            }
        }
        Ok(SearchResult { notes, cards })
    }
}

/// 调度阶段到排序权重的映射：重学最先，未知阶段垫底。
fn phase_rank(phase: &str) -> i64 {
    match phase {
        "relearning" => 0,
        "learning" => 1,
        "review" => 2,
        _ => 3,
    }
}

#[cfg(test)]
mod tests {
    use super::super::testutil;
    use crate::models::CardInput;

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
    /// 新卡片立即出现在复习队列中。
    async fn new_card_appears_in_queue() {
        let (storage, _config, _vault) = testutil::test_storage().await;
        storage
            .save_card(test_card("测试/笔记.md", "qa"))
            .await
            .expect("保存卡片失败");
        let queue = storage
            .get_review_queue(Some("测试/笔记.md"), false)
            .await
            .expect("查询队列失败");
        assert_eq!(queue.len(), 1);
        assert_eq!(queue[0].state, "new");
    }

    #[tokio::test]
    /// 搜索可以同时命中笔记正文与卡片内容。
    async fn search_hits_notes_and_cards() {
        let (storage, _config, _vault) = testutil::test_storage().await;
        storage
            .upsert_note_index("测试/笔记.md", "Rust 所有权", 1)
            .await
            .expect("写索引失败");
        let mut card = test_card("测试/笔记.md", "qa");
        card.back = "所有权规则".to_string();
        storage.save_card(card).await.expect("保存卡片失败");
        let result = storage.search("所有权").await.expect("搜索失败");
        assert_eq!(result.notes.len(), 1);
        assert_eq!(result.cards.len(), 1);
        assert!(result.notes[0].snippet.contains("<mark>所有权</mark>"));
    }

    #[tokio::test]
    /// 听写判定规范化后接受别名。
    async fn dictation_accepts_aliases() {
        let (storage, _config, _vault) = testutil::test_storage().await;
        let mut card = test_card("英语/词.md", "vocabulary");
        card.back = "Ephemeral".to_string();
        card.aliases = vec!["ephemeral".to_string()];
        let saved = storage.save_card(card).await.expect("保存卡片失败");
        let result = storage
            .check_dictation(&saved.id, "  EPHEMERAL ")
            .await
            .expect("听写判定失败");
        assert!(result.correct);
    }

    #[tokio::test]
    /// 删除卡片后队列不再包含它。
    async fn deleted_card_leaves_queue() {
        let (storage, _config, _vault) = testutil::test_storage().await;
        let saved = storage
            .save_card(test_card("测试/笔记.md", "qa"))
            .await
            .expect("保存卡片失败");
        storage.delete_card(&saved.id).await.expect("删除卡片失败");
        let queue = storage
            .get_review_queue(Some("测试/笔记.md"), true)
            .await
            .expect("查询队列失败");
        assert!(queue.is_empty());
    }
}
