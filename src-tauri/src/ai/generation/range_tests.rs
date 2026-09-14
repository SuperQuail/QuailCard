use super::*;
use serde_json::json;

/// 纯领域输入不依赖存储或网络。
fn input(text: &str) -> GenerationInput {
    GenerationInput {
        type_id: "qa".into(),
        study_mode_id: "self-review".into(),
        note_title: "测试".into(),
        source_text: text.into(),
        images: vec![],
        requested_count: -1,
        context: None,
    }
}

/// 稳定 ID 关联单个连续行区间。
fn item(id: &str, line: usize) -> Value {
    json!({"itemId":id,"keyword":id,"sourceRange":{"startLine":line,"endLine":line},"imageRef":null})
}

/// 落卡只提供 ID 而不重复抄写来源。
fn card(id: &str) -> Value {
    json!({"schema_version":1,"type_id":"qa","itemId":id,"fields":{"front":id,"back":"答案","detail":""}})
}

#[test]
/// 重复代码必须定位指定出现位置，保留制表符、尾部空格和 Unicode 坐标。
fn repeated_indented_source_uses_exact_snapshot_range() {
    let input = input(
        "😀
	code  
	code  ",
    );
    let mut session = GenerationSession::new(&input).unwrap();
    session
        .submit_plan(&input, json!({"items":[item("second",3)]}))
        .unwrap();
    session.accept(&input, card("second")).unwrap();
    assert!(session.pending_keywords().is_empty());
    let source = session.finish(None).cards.remove(0).source.unwrap();
    assert_eq!(source.excerpt, "	code  ");
    assert_eq!(
        source.from,
        "😀
	code  
"
        .encode_utf16()
        .count()
    );
}

#[test]
/// 有效兄弟保留，所有错误有稳定 ID，修复无需重发整表。
fn partial_plan_retains_siblings_and_repairs_failed_ids() {
    let input = input(
        "甲
乙
丙",
    );
    let mut session = GenerationSession::new(&input).unwrap();
    let summary = session
        .submit_plan(
            &input,
            json!({"items":[item("a",1),item("b",0),item("c",9)]}),
        )
        .unwrap();
    assert_eq!(
        summary
            .errors
            .iter()
            .map(|e| e.item_id.as_str())
            .collect::<Vec<_>>(),
        ["b", "c"]
    );
    assert_eq!(summary.planned, 3);
    assert!(summary.changed);
    session.accept(&input, card("a")).unwrap();
    assert_eq!(session.pending_keywords().len(), 2);
    assert_eq!(
        session.accept(&input, card("b")).unwrap_err().code,
        "INVALID_PLAN_ITEM"
    );
    let summary = session
        .submit_plan(&input, json!({"items":[item("b",2),item("c",3)]}))
        .unwrap();
    assert!(summary.errors.is_empty());
    assert_eq!(summary.emitted, 1);
    session.accept(&input, card("b")).unwrap();
    session.accept(&input, card("c")).unwrap();
    assert!(session.pending_keywords().is_empty());
}

#[test]
/// 已落地来源不能重写或删除，未落地错误允许显式删除。
fn emitted_entries_are_immutable() {
    let input = input(
        "甲
乙",
    );
    let mut session = GenerationSession::new(&input).unwrap();
    session
        .submit_plan(&input, json!({"items":[item("a",1),item("b",8)]}))
        .unwrap();
    session.accept(&input, card("a")).unwrap();
    let summary = session
        .submit_plan(
            &input,
            json!({"items":[item("a",2)],"removeItemIds":["a","b"]}),
        )
        .unwrap();
    assert_eq!(summary.errors.len(), 2);
    assert!(summary
        .errors
        .iter()
        .all(|e| e.code == "EMITTED_ITEM_IMMUTABLE"));
    assert!(session.pending_keywords().is_empty());
    assert_eq!(session.finish(None).cards[0].fields["source"], "甲");
}

#[test]
/// 超长范围在计划阶段报错，避免生成成功却无法采纳。
fn invalid_ranges_do_not_report_progress() {
    let input = input(&"a".repeat(4001));
    let mut session = GenerationSession::new(&input).unwrap();
    let summary = session
        .submit_plan(&input, json!({"items":[item("long",1)]}))
        .unwrap();
    assert!(!summary.changed);
    assert_eq!(summary.errors[0].code, "INVALID_SOURCE_RANGE");
    assert!(!summary.pending_items[0].valid);
}

#[test]
/// 后续调用的材料变化不能改变已固定的来源事实。
fn source_snapshot_is_immutable_and_preserves_crlf() {
    let mut input = input("甲\r\n\t乙  \r\n丙");
    let mut session = GenerationSession::new(&input).unwrap();
    input.source_text = "不同的新材料".into();
    session.submit_plan(&input,json!({"items":[{"itemId":"a","keyword":"a","sourceRange":{"startLine":1,"endLine":2}}]})).unwrap();
    session.accept(&input, card("a")).unwrap();
    assert_eq!(
        session.finish(None).cards[0].fields["source"],
        "甲\r\n\t乙  \r"
    );
}

#[test]
/// 图片 ID 与文件名均可精确匹配，不建立文本坐标。
fn explicit_image_refs_use_provided_images_only() {
    let mut input = input("材料");
    input.images.push(crate::models::GenerationImage {
        name: "图.png".into(),
        mime_type: "image/png".into(),
        data_base64: "eA==".into(),
    });
    let mut session = GenerationSession::new(&input).unwrap();
    let summary = session
        .submit_plan(
            &input,
            json!({"items":[
                {"itemId":"a","keyword":"a","imageRef":"image-1"},
                {"itemId":"b","keyword":"b","imageRef":"图.png"},
                {"itemId":"c","keyword":"c","imageRef":"missing"}
            ]}),
        )
        .unwrap();
    assert_eq!(summary.errors[0].item_id, "c");
    session.accept(&input, card("a")).unwrap();
    session.accept(&input, card("b")).unwrap();
    assert!(session
        .finish(None)
        .cards
        .iter()
        .all(|c| c.source.is_none() && c.fields["source"] == "图.png"));
}

#[test]
/// 删除重加和修改来回切换不能伪造进展。
fn remove_readd_and_oscillation_do_not_reset_progress() {
    let input = input(
        "甲
乙",
    );
    let mut session = GenerationSession::new(&input).unwrap();
    assert!(
        session
            .submit_plan(&input, json!({"items":[item("a",1)]}))
            .unwrap()
            .changed
    );
    assert!(
        !session
            .submit_plan(&input, json!({"removeItemIds":["a"],"items":[item("a",1)]}))
            .unwrap()
            .changed
    );
    assert!(
        !session
            .submit_plan(&input, json!({"items":[item("a",2)]}))
            .unwrap()
            .changed
    );
    assert!(
        !session
            .submit_plan(&input, json!({"items":[item("a",1)]}))
            .unwrap()
            .changed
    );
}

#[test]
/// 新的无 ID 坏项不能借用已修复占位的身份消失。
fn malformed_items_receive_collision_free_repair_ids() {
    let input = input("甲");
    let mut session = GenerationSession::new(&input).unwrap();
    let first = session.submit_plan(&input, json!({"items":[{}]})).unwrap();
    let id = first.errors[0].item_id.clone();
    session
        .submit_plan(&input, json!({"items":[item(&id,1)]}))
        .unwrap();
    session.accept(&input, card(&id)).unwrap();
    let next = session.submit_plan(&input, json!({"items":[{}]})).unwrap();
    assert_ne!(next.errors[0].item_id, id);
    assert_eq!(next.pending_items.len(), 1);
    assert!(!next.pending_items[0].valid);
}
