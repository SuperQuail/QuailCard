//! 验收引用来自已保存会话，文本结论只证明产出存在，不证明语义正确。
use super::*;

/// 向模型提供有界收据目录，避免让模型猜测内部 UUID。
pub(super) fn available(turn: &AgentTurn<'_>) -> Vec<Value> {
    turn.session.messages.iter().skip(start(turn)).rev().filter_map(|message| {
        if message.kind == "text" && message.role == "assistant" {
            return Some(json!({"receiptRef":format!("message:{}",message.id),"summary":message.content.chars().take(160).collect::<String>()}));
        }
        if message.kind == "exchange" {
            let entries = message.data["results"].as_array()?.iter().filter_map(|result| {
                let value: Value = serde_json::from_str(result["content"].as_str()?).ok()?;
                if !business_receipt(turn, message, result["tool_call_id"].as_str()?) { return None; }
                Some(json!({"receiptRef":format!("tool:{}",result["tool_call_id"].as_str()?),"path":value["result"]["path"],"successful":value["ok"] == true}))
            }).collect::<Vec<_>>();
            return Some(json!({"tools":entries}));
        }
        None
    }).take(24).collect()
}

/// 读取类收据复验当前 hash；写入类收据复验保存后的 hash，拒绝资料变化后的旧证据。
pub(super) fn exists(turn: &AgentTurn<'_>, reference: &str, success: bool) -> bool {
    if let Some(id) = reference.strip_prefix("message:") {
        return turn.session.messages.iter().skip(start(turn)).any(|m| {
            m.id == id && m.role == "assistant" && m.kind == "text" && !m.content.trim().is_empty()
        });
    }
    let Some(id) = reference.strip_prefix("tool:") else {
        return false;
    };
    turn.session
        .messages
        .iter()
        .skip(start(turn))
        .filter(|m| m.kind == "exchange")
        .any(|m| {
            m.data["results"].as_array().is_some_and(|results| {
                results.iter().any(|result| {
                    if result["tool_call_id"] != id || (success && !business_receipt(turn, m, id)) {
                        return false;
                    }
                    let Some(content) = result["content"].as_str() else {
                        return false;
                    };
                    let Ok(value) = serde_json::from_str::<Value>(content) else {
                        return false;
                    };
                    if success && value["ok"] != true {
                        return false;
                    }
                    if !success {
                        return value.get("ok").is_some();
                    }
                    let data = &value["result"];
                    let expected = data["hash"].as_str().or_else(|| data["afterHash"].as_str());
                    if let (Some(path), Some(hash)) = (data["path"].as_str(), expected) {
                        if !turn.input.selected_paths.is_empty()
                            && !turn.input.selected_paths.iter().any(|p| p == path)
                        {
                            return false;
                        }
                        return turn
                            .ports
                            .repository
                            .read(path)
                            .is_ok_and(|note| note["hash"] == hash);
                    }
                    true
                })
            })
        })
}

/// 最近一次目标定义变更之前的产出不作为当前目标的新验收收据。
fn start(turn: &AgentTurn<'_>) -> usize {
    let Some(goal) = &turn.session.goal else {
        return 0;
    };
    turn.session
        .messages
        .iter()
        .rposition(|m| m.data["goalReset"] == true && m.data["goal"]["id"] == goal.id)
        .map(|index| index + 1)
        .unwrap_or(0)
}

/// 状态管理工具不证明业务完成；拒绝拿 get_goal 或 update_plan 自我背书。
fn business_receipt(
    turn: &AgentTurn<'_>,
    exchange: &crate::agent_models::AgentMessage,
    id: &str,
) -> bool {
    exchange.data["calls"].as_array().is_some_and(|calls| {
        calls.iter().any(|call| {
            call["id"] == id
                && call["name"].as_str().is_some_and(|name| {
                    turn.registered.iter().any(|tool| tool.spec.name == name)
                        && !["remember", "start_review", "generate_cards"].contains(&name)
                })
        })
    })
}

/// 待采纳关联当前目标定义，而非所有历史草稿；采纳事实跨重启保留。
pub(in crate::services::agent) fn pending_drafts(turn: &AgentTurn<'_>) -> bool {
    let Some(goal) = &turn.session.goal else {
        return false;
    };
    turn.session
        .messages
        .iter()
        .skip(start(turn))
        .any(|message| {
            message.kind == "drafts"
                && message.data["goalId"] == goal.id
                && message.data["adoptionResolved"] != true
                && message.data["cards"]
                    .as_array()
                    .is_some_and(|cards| !cards.is_empty())
        })
}
