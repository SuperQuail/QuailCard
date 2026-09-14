//! Agent 卡片管理适配器：只按笔记列出与删除卡片，不向用例层暴露卡片存储实现。
use crate::{
    error::CommandError,
    models::NoteCard,
    services::agent_ports::{AgentCards, AgentFuture},
    storage::Storage,
};
use serde_json::{json, Value};
use tauri::Manager;

/// 单次列出的卡片上限：超出时用 total 告知模型，避免整篇笔记的卡片灌满上下文。
const CARD_LIMIT: usize = 200;
/// 摘要字段上限：列表只给识别卡片所需的最小切片，完整正文仍由卡片面板查看。
const SNIPPET_LIMIT: usize = 240;

pub(crate) struct CardsAdapter {
    pub app: tauri::AppHandle,
}

impl AgentCards for CardsAdapter {
    /// 列出笔记卡片的安全摘要；超限只回传前 N 条并标记 truncated。
    fn list<'a>(&'a self, note_path: &'a str) -> AgentFuture<'a, Value> {
        Box::pin(async move {
            let cards = self
                .app
                .state::<Storage>()
                .list_note_cards(note_path)
                .await?;
            let total = cards.len();
            let items = cards
                .iter()
                .take(CARD_LIMIT)
                .map(summary)
                .collect::<Vec<_>>();
            Ok(json!({
                "path": note_path,
                "total": total,
                "truncated": total > CARD_LIMIT,
                "items": items,
            }))
        })
    }

    /// 删除前先确认卡片属于该笔记，防止用范围允许的路径删除范围外的卡片。
    fn delete<'a>(&'a self, note_path: &'a str, card_id: &'a str) -> AgentFuture<'a, Value> {
        Box::pin(async move {
            let storage = self.app.state::<Storage>();
            let card = storage
                .list_note_cards(note_path)
                .await?
                .into_iter()
                .find(|card| card.id == card_id)
                .ok_or_else(|| {
                    CommandError::new(
                        "CARD_NOT_FOUND",
                        "该笔记下没有这张卡片，请先用 list_cards 确认 cardId",
                    )
                })?;
            storage.delete_card(card_id).await?;
            Ok(json!({
                "path": note_path,
                "id": card.id,
                "kind": card.kind,
                "front": snippet(&card.front),
            }))
        })
    }
}

/// 卡片摘要只保留识别所需字段，正反面过长的部分截断。
fn summary(card: &NoteCard) -> Value {
    json!({
        "id": card.id,
        "kind": card.kind,
        "front": snippet(&card.front),
        "back": snippet(&card.back),
    })
}

/// 按字符截断，不拆开 UTF-8 字节。
fn snippet(text: &str) -> String {
    if text.chars().count() <= SNIPPET_LIMIT {
        return text.to_string();
    }
    let mut shortened = text.chars().take(SNIPPET_LIMIT).collect::<String>();
    shortened.push('…');
    shortened
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 摘要截断按字符进行，长卡片不会撑爆列表上下文。
    #[test]
    fn snippet_truncates_by_character() {
        assert_eq!(snippet("短"), "短");
        let long = "字".repeat(SNIPPET_LIMIT + 10);
        let shortened = snippet(&long);
        assert_eq!(shortened.chars().count(), SNIPPET_LIMIT + 1);
        assert!(shortened.ends_with('…'));
    }
}
