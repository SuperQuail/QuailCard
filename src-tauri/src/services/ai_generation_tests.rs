use super::*;
use crate::ai::{ToolArgumentError, ToolCallResult};

/// 创建服务轮次测试使用的问答输入。
fn qa_input(count: i32) -> GenerationInput {
    GenerationInput {
        type_id: "qa".to_string(),
        study_mode_id: "self-review".to_string(),
        note_title: "测试".to_string(),
        source_text: "材料".to_string(),
        images: vec![],
        requested_count: count,
    }
}

/// 创建合法单卡参数。
fn qa_arguments(front: &str, back: &str) -> Value {
    json!({
        "schema_version": 1,
        "type_id": "qa",
        "fields": { "front": front, "back": back, "detail": "来源" }
    })
}

/// 创建带稳定调用标识的工具调用。
fn call(id: &str, name: &str, arguments: ToolArguments) -> ToolCallResult {
    ToolCallResult {
        id: id.to_string(),
        item_id: None,
        name: name.to_string(),
        arguments,
    }
}

/// 从轮次历史中提取指定调用的工具结果。
fn tool_result<'a>(history: &'a [ToolMessage], id: &str) -> &'a str {
    history
        .iter()
        .find_map(|message| match message {
            ToolMessage::ToolResult {
                id: result_id,
                content,
            } if result_id == id => Some(content.as_str()),
            _ => None,
        })
        .expect("缺少工具结果")
}

#[tokio::test]
/// 同轮多个 emit_card 调用均被独立接收。
async fn accepts_multiple_emit_card_calls() {
    let dictionary = Dictionary::connect_memory_for_test().await;
    let input = qa_input(2);
    let mut session = GenerationSession::new(&input).unwrap();
    let batch = ToolCallBatch {
        calls: vec![
            call(
                "one",
                "emit_card",
                ToolArguments::Valid(qa_arguments("一", "答一")),
            ),
            call(
                "two",
                "emit_card",
                ToolArguments::Valid(qa_arguments("二", "答二")),
            ),
        ],
        continuation_items: vec![],
    };
    let round = process_generation_round(&dictionary, &input, &mut session, batch, "trace")
        .await
        .unwrap();
    assert_eq!(session.generated(), 2);
    assert_eq!(
        serde_json::from_str::<Value>(tool_result(&round.history, "two")).unwrap()["remaining"],
        0
    );
}

#[tokio::test]
/// 一个无效 JSON 调用不会丢弃合法兄弟调用并明确要求重试。
async fn preserves_valid_sibling_of_malformed_call() {
    let dictionary = Dictionary::connect_memory_for_test().await;
    let input = qa_input(2);
    let mut session = GenerationSession::new(&input).unwrap();
    let batch = ToolCallBatch {
        calls: vec![
            call(
                "bad",
                "emit_card",
                ToolArguments::Invalid(ToolArgumentError {
                    line: 1,
                    column: 8,
                    category: "eof",
                }),
            ),
            call(
                "good",
                "emit_card",
                ToolArguments::Valid(qa_arguments("问题", "答案")),
            ),
        ],
        continuation_items: vec![],
    };
    let round = process_generation_round(&dictionary, &input, &mut session, batch, "trace")
        .await
        .unwrap();
    let error: Value = serde_json::from_str(tool_result(&round.history, "bad")).unwrap();
    assert_eq!(session.generated(), 1);
    assert_eq!(error["error"]["code"], "INVALID_JSON");
    assert_eq!(error["action"], "retry");
    assert!(error["instruction"]
        .as_str()
        .is_some_and(|value| value.contains("不要重复已经成功")));
}

#[tokio::test]
/// 重复卡片获得独立错误且已接收卡片保持不变。
async fn rejects_duplicate_with_tool_feedback() {
    let dictionary = Dictionary::connect_memory_for_test().await;
    let input = qa_input(2);
    let mut session = GenerationSession::new(&input).unwrap();
    session
        .accept(&input, qa_arguments("问题", "答案"))
        .unwrap();
    let batch = ToolCallBatch {
        calls: vec![call(
            "duplicate",
            "emit_card",
            ToolArguments::Valid(qa_arguments("问题", "答案")),
        )],
        continuation_items: vec![],
    };
    let round = process_generation_round(&dictionary, &input, &mut session, batch, "trace")
        .await
        .unwrap();
    let result: Value = serde_json::from_str(tool_result(&round.history, "duplicate")).unwrap();
    assert_eq!(result["error"]["code"], "DUPLICATE_CARD");
    assert_eq!(session.generated(), 1);
}

#[tokio::test]
/// 自动数量同轮先接收卡片再处理结束调用。
async fn processes_cards_before_auto_finish() {
    let dictionary = Dictionary::connect_memory_for_test().await;
    let input = qa_input(-1);
    let mut session = GenerationSession::new(&input).unwrap();
    let batch = ToolCallBatch {
        calls: vec![
            call(
                "finish",
                "finish_generation",
                ToolArguments::Valid(json!({})),
            ),
            call(
                "card",
                "emit_card",
                ToolArguments::Valid(qa_arguments("问题", "答案")),
            ),
        ],
        continuation_items: vec![],
    };
    let round = process_generation_round(&dictionary, &input, &mut session, batch, "trace")
        .await
        .unwrap();
    assert!(round.finish_auto);
    assert_eq!(session.generated(), 1);
}

#[tokio::test]
/// 同轮词典查询会延后卡片直到模型看到查询结果。
async fn defers_same_round_card_when_lookup_is_called() {
    let dictionary = Dictionary::connect_memory_for_test().await;
    let mut input = qa_input(1);
    input.type_id = "vocabulary".to_string();
    input.study_mode_id = "dictation".to_string();
    let mut session = GenerationSession::new(&input).unwrap();
    let card = json!({
        "schema_version": 1, "type_id": "vocabulary",
        "fields": { "front": "说", "back": "speak", "detail": "", "example": "Speak.", "aliases": "" }
    });
    let batch = ToolCallBatch {
        calls: vec![
            call(
                "lookup",
                "lookup_words",
                ToolArguments::Valid(json!({ "words": ["speak"] })),
            ),
            call("card", "emit_card", ToolArguments::Valid(card)),
        ],
        continuation_items: vec![],
    };
    let round = process_generation_round(&dictionary, &input, &mut session, batch, "trace")
        .await
        .unwrap();
    let deferred: Value = serde_json::from_str(tool_result(&round.history, "card")).unwrap();
    assert_eq!(deferred["error"]["code"], "LOOKUP_RESULT_PENDING");
    assert_eq!(session.generated(), 0);
}

#[test]
/// 达到安全轮次上限时有卡返回警告，无卡返回安全错误。
fn handles_generation_limit_with_partial_result() {
    let input = qa_input(2);
    let empty = GenerationSession::new(&input).unwrap();
    let error = finish_at_limit(empty, "limit").unwrap_err();
    assert_eq!(error.code, "PROVIDER_GENERATION_LIMIT_REACHED");

    let mut partial = GenerationSession::new(&input).unwrap();
    partial
        .accept(&input, qa_arguments("问题", "答案"))
        .unwrap();
    let result = finish_at_limit(partial, "limit").unwrap();
    assert_eq!(result.cards.len(), 1);
    assert_eq!(result.warnings, ["limit"]);
}
