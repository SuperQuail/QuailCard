//! 回归模型批次中的局部失败：有效条目无需整表重交，落卡只引用稳定 ID。
use super::*;
use crate::{
    ai::{generation_tools, ToolCallResult},
    dictionary::DictionaryEntry,
    services::generation_ports::PortFuture,
};

struct Dictionary;
impl DictionaryLookup for Dictionary {
    /// 测试不访问本地词库或外部服务。
    fn lookup<'a>(&'a self, _: &'a str) -> PortFuture<'a, Option<DictionaryEntry>> {
        Box::pin(async { Ok(None) })
    }
}

/// 固定材料包含缩进、空行和重复片段，行号由同一快照定义。
fn input() -> GenerationInput {
    GenerationInput {
        type_id: "qa".into(),
        study_mode_id: "self-review".into(),
        note_title: "代码".into(),
        source_text: "反弹\n\tif x > 0:\n\t\tdirection = -1\n\n重复\n重复".into(),
        images: vec![],
        requested_count: -1,
        context: None,
    }
}

/// 每次仅提交一个工具便于检查对应的结构化反馈。
async fn run(
    session: &mut GenerationSession,
    input: &GenerationInput,
    name: &str,
    args: Value,
) -> (Value, bool, Option<String>) {
    let tools = GenerationTools::build(generation_tools(input).unwrap()).unwrap();
    let batch = ToolCallBatch {
        calls: vec![ToolCallResult {
            id: "call".into(),
            item_id: None,
            name: name.into(),
            arguments: ToolArguments::Valid(args),
        }],
        continuation_items: vec![],
    };
    let result = process_generation_round(
        &Dictionary,
        input,
        session,
        batch,
        &tools,
        &mut HashSet::new(),
        &GenerationControl::new("test".into()),
    )
    .await;
    let value = result
        .history
        .iter()
        .find_map(|item| match item {
            ToolMessage::ToolResult { content, .. } => Some(serde_json::from_str(content).unwrap()),
            _ => None,
        })
        .unwrap();
    (value, result.progressed, result.finish_reason)
}

/// 清单工具使用固定 ID，局部重试不用复制已经成功的条目。
fn item(id: &str, keyword: &str, start: usize, end: usize) -> Value {
    json!({"itemId":id,"keyword":keyword,"sourceRange":{"startLine":start,"endLine":end},"imageRef":null})
}

/// 卡片内容与来源关联分离，模型无法用第二份摘录改变来源。
fn card(id: &str, question: &str) -> Value {
    json!({"itemId":id,"schema_version":1,"type_id":"qa","fields":{"front":question,"back":"答案","detail":""}})
}

#[tokio::test]
/// 一次报告多个错误、保留有效项，修复一个条目后仍阻止提前结束。
async fn partial_plan_repairs_and_emits_by_id() {
    let input = input();
    let mut session = GenerationSession::new(&input).unwrap();
    let (value, progressed, _) = run(
        &mut session,
        &input,
        "plan_cards",
        json!({"items":[item("p1","反弹",2,3),item("p2","重复",30,40),item("p3","空行",0,1)]}),
    )
    .await;
    assert_eq!(value["ok"], false);
    assert_eq!(value["errors"].as_array().unwrap().len(), 2);
    assert!(progressed);
    assert_eq!(value["pendingItems"].as_array().unwrap().len(), 3);
    let (value, progressed, _) =
        run(&mut session, &input, "emit_card", card("p1", "如何反弹？")).await;
    assert_eq!(value["ok"], true);
    assert!(progressed);
    let (value, _, reason) = run(
        &mut session,
        &input,
        "finish_generation",
        json!({"reason":"完成"}),
    )
    .await;
    assert_eq!(value["ok"], false);
    assert!(reason.is_none());
    let (value, _, _) = run(
        &mut session,
        &input,
        "plan_cards",
        json!({"items":[item("p2","重复",6,6),item("p3","空行",1,1)]}),
    )
    .await;
    assert_eq!(value["ok"], true);
    assert_eq!(value["emitted"], 1);
    assert_eq!(value["planned"], 3);
    for (id, question) in [("p2", "哪一段重复？"), ("p3", "标题是什么？")] {
        assert_eq!(
            run(&mut session, &input, "emit_card", card(id, question))
                .await
                .0["ok"],
            true
        );
    }
    let (value, _, reason) = run(
        &mut session,
        &input,
        "finish_generation",
        json!({"reason":"完成"}),
    )
    .await;
    assert_eq!(value["ok"], true);
    assert!(reason.is_some());
    let result = session.finish(None);
    assert_eq!(
        result.cards[0].fields["source"],
        "\tif x > 0:\n\t\tdirection = -1"
    );
    let source = result.cards[1].source.as_ref().unwrap();
    assert_eq!(
        source.from,
        input
            .source_text
            .rfind("重复")
            .map(|offset| input.source_text[..offset].encode_utf16().count())
            .unwrap()
    );
}

#[tokio::test]
/// 相同清单重复提交不算进展，也不能重复落卡。
async fn repeated_plan_is_not_progress() {
    let input = input();
    let mut session = GenerationSession::new(&input).unwrap();
    let plan = json!({"items":[item("p1","反弹",2,3)]});
    assert!(
        run(&mut session, &input, "plan_cards", plan.clone())
            .await
            .1
    );
    assert!(!run(&mut session, &input, "plan_cards", plan).await.1);
    assert_eq!(
        run(&mut session, &input, "emit_card", card("p1", "如何反弹？"))
            .await
            .0["ok"],
        true
    );
    let (value, progressed, _) =
        run(&mut session, &input, "emit_card", card("p1", "如何反弹？")).await;
    assert_eq!(value["error"]["code"], "ALREADY_EMITTED");
    assert!(!progressed);
}

#[tokio::test]
/// 误列考点可显式撤回，避免为了修复重复占位而虚构新考点。
async fn removes_only_pending_items_explicitly() {
    let input = input();
    let mut session = GenerationSession::new(&input).unwrap();
    let plan = json!({"items":[item("p1","反弹",2,3),item("p2","重复",90,90)]});
    run(&mut session, &input, "plan_cards", plan).await;
    run(&mut session, &input, "emit_card", card("p1", "如何反弹？")).await;
    let (value, _, _) = run(
        &mut session,
        &input,
        "plan_cards",
        json!({"items":[],"removeItemIds":["p2"]}),
    )
    .await;
    assert_eq!(value["ok"], true);
    assert_eq!(value["planned"], 1);
    assert_eq!(value["emitted"], 1);
    let (value, _, _) = run(
        &mut session,
        &input,
        "plan_cards",
        json!({"items":[],"removeItemIds":["p1"]}),
    )
    .await;
    assert_eq!(value["ok"], false);
    assert_eq!(session.generated(), 1);
    assert_eq!(
        run(
            &mut session,
            &input,
            "finish_generation",
            json!({"reason":"完成"})
        )
        .await
        .0["ok"],
        true
    );
}

#[tokio::test]
/// 格式错误的整批清单没有占位时，也不能用同轮结束调用掩盖失败。
async fn failed_plan_cannot_finish_in_same_batch() {
    let input = input();
    let mut session = GenerationSession::new(&input).unwrap();
    let tools = GenerationTools::build(generation_tools(&input).unwrap()).unwrap();
    let calls = [
        ("plan_cards", json!({"items":[]})),
        ("finish_generation", json!({"reason":"完成"})),
    ]
    .into_iter()
    .map(|(name, args)| ToolCallResult {
        id: name.into(),
        item_id: None,
        name: name.into(),
        arguments: ToolArguments::Valid(args),
    })
    .collect();
    let round = process_generation_round(
        &Dictionary,
        &input,
        &mut session,
        ToolCallBatch {
            calls,
            continuation_items: vec![],
        },
        &tools,
        &mut HashSet::new(),
        &GenerationControl::new("test".into()),
    )
    .await;
    assert!(round.finish_reason.is_none());
    assert!(!round.progressed);
    let result: Value = round
        .history
        .iter()
        .find_map(|message| match message {
            ToolMessage::ToolResult { id, content } if id == "finish_generation" => {
                Some(serde_json::from_str(content).unwrap())
            }
            _ => None,
        })
        .unwrap();
    assert_eq!(result["error"]["code"], "CORRECTIONS_PENDING");
}
