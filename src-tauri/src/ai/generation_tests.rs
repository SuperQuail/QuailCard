use super::*;

/// 创建生成测试使用的输入。
fn test_input(type_id: &str, count: i32) -> GenerationInput {
    GenerationInput {
        type_id: type_id.to_string(),
        study_mode_id: if type_id == "vocabulary" {
            "dictation".to_string()
        } else {
            "self-review".to_string()
        },
        note_title: "测试".to_string(),
        source_text: "学习材料".to_string(),
        images: vec![],
        requested_count: count,
    }
}

/// 创建一张合法问答卡的工具参数。
fn qa_card(front: &str, back: &str) -> Value {
    json!({
        "schema_version": 1,
        "type_id": "qa",
        "fields": { "front": front, "back": back, "detail": "来源" }
    })
}

#[test]
/// 会话逐张接收多个调用并在固定数量完成时结束。
fn accepts_multiple_cards_for_fixed_count() {
    let input = test_input("qa", 2);
    let mut session = GenerationSession::new(&input).expect("创建会话失败");
    session.accept(&input, qa_card("问题一", "答案一")).unwrap();
    session.accept(&input, qa_card("问题二", "答案二")).unwrap();
    assert!(session.fixed_complete());
    assert_eq!(session.finish(None).cards.len(), 2);
}

#[test]
/// 重复卡片被独立拒绝且不影响已经接收的卡片。
fn rejects_duplicate_card_independently() {
    let input = test_input("qa", 2);
    let mut session = GenerationSession::new(&input).expect("创建会话失败");
    session.accept(&input, qa_card("问题", "答案")).unwrap();
    let error = session
        .accept(&input, qa_card(" 问题 ", "答案"))
        .unwrap_err();
    assert_eq!(error.code, "DUPLICATE_CARD");
    assert_eq!(session.generated(), 1);
}

#[test]
/// 固定数量完成后额外调用会收到数量上限错误。
fn rejects_calls_beyond_fixed_target() {
    let input = test_input("qa", 1);
    let mut session = GenerationSession::new(&input).expect("创建会话失败");
    session.accept(&input, qa_card("问题一", "答案一")).unwrap();
    let error = session
        .accept(&input, qa_card("问题二", "答案二"))
        .unwrap_err();
    assert_eq!(error.code, "COUNT_LIMIT_REACHED");
    assert_eq!(session.generated(), 1);
}

#[test]
/// AI 问答缺少判定要点时拒绝单卡调用。
fn rejects_ai_review_without_rubric() {
    let mut input = test_input("qa", 1);
    input.study_mode_id = "ai-review".to_string();
    let mut session = GenerationSession::new(&input).expect("创建会话失败");
    let error = session.accept(&input, qa_card("问题", "答案")).unwrap_err();
    assert_eq!(error.code, "MISSING_FIELD");
}

#[test]
/// 单卡 Schema 不再包含 cards 数组并关闭额外属性。
fn builds_single_card_schema_without_cards_array() {
    let tool = generation_tool(&test_input("qa", 2)).expect("创建生成工具失败");
    assert_eq!(tool.name, "emit_card");
    assert!(tool.input_schema["properties"].get("cards").is_none());
    assert_eq!(
        tool.input_schema["properties"]["fields"]["additionalProperties"],
        false
    );
}

#[test]
/// 问答卡不暴露词典工具，自动数量额外暴露结束工具。
fn selects_tools_for_generation_mode() {
    let fixed = generation_tools(&test_input("qa", 2)).unwrap();
    assert_eq!(
        fixed.iter().map(|tool| tool.name).collect::<Vec<_>>(),
        ["emit_card"]
    );
    let automatic = generation_tools(&test_input("qa", -1)).unwrap();
    assert_eq!(
        automatic.iter().map(|tool| tool.name).collect::<Vec<_>>(),
        ["emit_card", "finish_generation"]
    );
}

#[test]
/// 自动数量至少接收一张卡后才允许结束。
fn auto_finish_requires_a_card() {
    let input = test_input("qa", -1);
    let mut session = GenerationSession::new(&input).expect("创建会话失败");
    assert!(!session.can_finish_auto());
    session.accept(&input, qa_card("问题", "答案")).unwrap();
    assert!(session.can_finish_auto());
}

#[test]
/// 纯图片材料通过输入校验。
fn accepts_image_only_generation() {
    let mut input = test_input("qa", 1);
    input.source_text.clear();
    input.images.push(crate::models::GenerationImage {
        name: "note.png".to_string(),
        mime_type: "image/png".to_string(),
        data_base64: general_purpose::STANDARD.encode(b"image"),
    });
    assert!(validate_generation_input(&input).is_ok());
}

#[test]
/// 单词卡 front 提示词与 Schema 共用同一份显式格式说明。
fn vocabulary_front_format_is_explicit_and_consistent() {
    let profile = generation_profile("vocabulary").expect("读取单词卡配置失败");
    assert!(profile.field_instruction.contains(VOCABULARY_FRONT_FORMAT));
    let tool = generation_tool(&test_input("vocabulary", 1)).expect("创建生成工具失败");
    assert_eq!(
        tool.input_schema["properties"]["fields"]["properties"]["front"]["description"],
        VOCABULARY_FRONT_FORMAT
    );
}
