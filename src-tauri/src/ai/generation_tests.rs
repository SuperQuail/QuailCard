use super::*;
use base64::{engine::general_purpose, Engine as _};
use serde_json::json;

/// 使用真实输入边界构造可复用的领域场景。
fn test_input(type_id: &str, count: i32) -> GenerationInput {
    GenerationInput {
        type_id: type_id.to_string(),
        study_mode_id: if type_id == "vocabulary" {
            "dictation"
        } else {
            "self-review"
        }
        .to_string(),
        note_title: "测试".to_string(),
        source_text: "学习材料".to_string(),
        images: vec![],
        requested_count: count,
        context: None,
    }
}

/// 卡片必须携带可定位的原文摘录。
fn qa_card(front: &str, back: &str) -> Value {
    json!({"schema_version":1,"type_id":"qa","fields":{"front":front,"back":back,"detail":"来源"},"source":"学习材料"})
}

#[test]
/// 同一问题即使答案不同，全角大小写和空白差异也不能绕过判重。
fn rejects_question_identity_duplicates() {
    let input = test_input("qa", 5);
    let mut session = GenerationSession::new(&input).unwrap();
    session
        .accept(&input, qa_card("  Ｈｅｌｌｏ　 world ", "答案一"))
        .unwrap();
    let error = session
        .accept(&input, qa_card("hello WORLD", "答案二"))
        .unwrap_err();
    assert_eq!(error.code, "DUPLICATE_CARD");
    assert_eq!(session.generated(), 1);
}

#[test]
/// 单词以待默写词条判重，不同释义仍属同一词条。
fn vocabulary_identity_uses_answer_and_preserves_fields() {
    let input = test_input("vocabulary", 5);
    let mut session = GenerationSession::new(&input).unwrap();
    let card = json!({"schema_version":1,"type_id":"vocabulary","fields":{"front":"v. 说","back":"ＳＰＥＡＫ","detail":"/spiːk/","example":"Speak clearly.","aliases":"spoke、spoken"},"source":"学习材料"});
    session.accept(&input, card.clone()).unwrap();
    let mut duplicate = card;
    duplicate["fields"]["front"] = json!("v. 讲话");
    duplicate["fields"]["back"] = json!("speak");
    assert_eq!(
        session.accept(&input, duplicate).unwrap_err().code,
        "DUPLICATE_CARD"
    );
    let card = session.finish(None).cards.remove(0);
    assert_eq!(card.fields["aliases"], "spoke、spoken");
    assert_eq!(card.fields["detail"], "/spiːk/");
    assert!(Uuid::parse_str(&card.draft_id).is_ok());
}

#[test]
/// 唯一摘录使用 UTF-16 坐标，重复摘录仅保留文本和警告。
fn source_handles_unicode_and_ambiguity() {
    let mut input = test_input("qa", 2);
    input.source_text = "😀学习材料".to_string();
    let mut session = GenerationSession::new(&input).unwrap();
    session.accept(&input, qa_card("问", "答")).unwrap();
    let source = session.finish(None).cards.remove(0).source.unwrap();
    assert_eq!((source.from, source.to), (2, 6));
    input.source_text = "学习材料，学习材料".to_string();
    let mut session = GenerationSession::new(&input).unwrap();
    session.accept(&input, qa_card("问", "答")).unwrap();
    let result = session.finish(None);
    assert!(result.cards[0].source.is_none());
    assert_eq!(result.cards[0].fields["source"], "学习材料");
    assert!(!result.warnings.is_empty());
}

#[test]
/// 不在材料中的文本会被反馈纠正，不能伪造定位。
fn rejects_unsupported_excerpt() {
    let input = test_input("qa", 2);
    let mut session = GenerationSession::new(&input).unwrap();
    let mut card = qa_card("问", "答");
    card["source"] = json!("不存在的内容");
    assert_eq!(
        session.accept(&input, card).unwrap_err().code,
        "SOURCE_NOT_FOUND"
    );
}

#[test]
/// 纯图片来源保留说明但绝不生成文本坐标。
fn image_only_source_has_no_text_coordinates() {
    let mut input = test_input("qa", 1);
    input.source_text.clear();
    input.images.push(crate::models::GenerationImage {
        name: "note.png".to_string(),
        mime_type: "image/png".to_string(),
        data_base64: general_purpose::STANDARD.encode(b"image"),
    });
    let mut session = GenerationSession::new(&input).unwrap();
    let mut card = qa_card("问", "答");
    card["source"] = json!("note.png 中的公式");
    session.accept(&input, card).unwrap();
    assert!(session.finish(None).cards[0].source.is_none());
}

#[test]
/// 评分要点不能为空且所有辅助字段都受到长度约束。
fn checks_rubric_and_optional_field_limits() {
    let mut input = test_input("qa", 1);
    input.study_mode_id = "ai-review".to_string();
    let mut session = GenerationSession::new(&input).unwrap();
    assert_eq!(
        session
            .accept(&input, qa_card("问", "答"))
            .unwrap_err()
            .code,
        "MISSING_FIELD"
    );
    let mut card = qa_card("问", "答");
    card["fields"]["rubric"] = json!("要点一、要点二");
    card["fields"]["detail"] = json!("字".repeat(4001));
    assert_eq!(
        session.accept(&input, card.clone()).unwrap_err().code,
        "FIELD_TOO_LONG"
    );
    card["fields"]["detail"] = json!("");
    session.accept(&input, card).unwrap();
    assert_eq!(
        session.finish(None).cards[0].fields["rubric"],
        "要点一、要点二"
    );
}

#[test]
/// 数量上限到达后不再接受更多卡片。
fn enforces_count_upper_bound() {
    let input = test_input("qa", 1);
    let mut session = GenerationSession::new(&input).unwrap();
    session.accept(&input, qa_card("问一", "答")).unwrap();
    assert!(session.fixed_complete());
    assert_eq!(
        session
            .accept(&input, qa_card("问二", "答"))
            .unwrap_err()
            .code,
        "COUNT_LIMIT_REACHED"
    );
}

#[test]
/// 清单可校验：来源必须逐字存在、keyword 唯一，整表重发保留已完成状态。
fn plan_validates_sources_and_keeps_emitted_state() {
    let input = test_input("qa", -1);
    let mut session = GenerationSession::new(&input).unwrap();
    let plan = json!({"items":[{"source":"学习材料","keyword":"考点一"},{"source":"学习材料","keyword":"考点二"}]});
    let summary = session.submit_plan(&input, plan.clone()).unwrap();
    assert_eq!((summary.planned, summary.emitted), (2, 0));
    assert_eq!(session.pending_keywords(), ["考点一", "考点二"]);
    let index = session.plan_slot("学习材料").unwrap();
    session.mark_plan_emitted(index);
    let summary = session.submit_plan(&input, plan).unwrap();
    assert_eq!((summary.planned, summary.emitted), (2, 1));
    assert_eq!(session.pending_keywords(), ["考点二"]);
    assert_eq!(
        session
            .submit_plan(
                &input,
                json!({"items":[{"source":"学习材料","keyword":"x"},{"source":"学习材料","keyword":"X"}]})
            )
            .unwrap()
            .errors[0].code,
        "DUPLICATE_PLAN_ITEM"
    );
    assert_eq!(
        session
            .submit_plan(&input, json!({"items":[{"source":"不存在","keyword":"y"}]}))
            .unwrap()
            .errors[0]
            .code,
        "SOURCE_NOT_FOUND"
    );
}

#[test]
/// 不限量模式不设数量上限，剩余量无意义，由模型调用 finish_generation 结束。
fn unlimited_mode_never_hits_count_limit() {
    let input = test_input("qa", -1);
    let mut session = GenerationSession::new(&input).unwrap();
    session.accept(&input, qa_card("问一", "答")).unwrap();
    session.accept(&input, qa_card("问二", "答")).unwrap();
    assert!(!session.fixed_complete());
    assert_eq!(session.remaining(), None);
    assert_eq!(session.generated(), 2);
    assert!(session.accept(&input, qa_card("问三", "答")).is_ok());
}

#[test]
/// 不限量时提示词不写死张数；两种模式都要求分批逐张提交。
fn prompt_states_quantity_and_batching() {
    let limited = build_generation_prompt(&test_input("qa", 5)).unwrap();
    assert!(limited.1.contains("最多生成 5 张"));
    assert!(limited.0.contains("优先每轮 1-3 张"));
    let unlimited = build_generation_prompt(&test_input("qa", -1)).unwrap();
    assert!(unlimited.1.contains("数量不设上限"));
    assert!(!unlimited.1.contains("最多生成"));
    assert!(unlimited
        .0
        .contains("先用 plan_cards 提交本次要覆盖的完整考点清单"));
}

#[test]
/// 固定数量和自动数量都能明确提前结束，工具字段与指令一致。
fn schema_and_prompts_allow_early_completion() {
    for count in [5, -1] {
        let input = test_input("qa", count);
        let tools = generation_tools(&input).unwrap();
        assert_eq!(
            tools.iter().map(|tool| tool.name).collect::<Vec<_>>(),
            [
                "read_generation_material",
                "plan_cards",
                "emit_card",
                "finish_generation"
            ]
        );
        let emit = tools
            .iter()
            .find(|tool| tool.name == "emit_card")
            .expect("emit_card 必须注册");
        assert!(emit.schema["required"]
            .as_array()
            .unwrap()
            .contains(&json!("itemId")));
        assert!(build_generation_prompt(&input)
            .unwrap()
            .0
            .contains("说明零张"));
    }
    let input = test_input("vocabulary", 5);
    let tool = tools::generation_tool(&input).unwrap();
    assert_eq!(
        tool.schema["properties"]["fields"]["properties"]["front"]["description"],
        profile::VOCABULARY_FRONT_FORMAT
    );
    assert!(build_generation_prompt(&input)
        .unwrap()
        .1
        .contains(profile::VOCABULARY_FRONT_FORMAT));
}
