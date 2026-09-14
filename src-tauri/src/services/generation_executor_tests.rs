use std::{
    collections::VecDeque,
    sync::{
        atomic::{AtomicUsize, Ordering},
        Mutex,
    },
};

use serde_json::{json, Value};

use super::*;
use crate::{
    ai::{ToolArguments, ToolCallBatch, ToolCallResult},
    dictionary::DictionaryEntry,
    services::generation_ports::PortFuture,
};

enum Step {
    Batch(ToolCallBatch),
    Error,
    Cancel(GenerationControl),
}
struct Model {
    steps: Mutex<VecDeque<Step>>,
    calls: AtomicUsize,
}

impl GenerationModel for Model {
    /// 假模型直接提供工具批次，测试不访问付费服务。
    fn call<'a>(&'a self, _: MultiToolRequest<'a>) -> PortFuture<'a, ToolCallBatch> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        let step = self
            .steps
            .lock()
            .unwrap()
            .pop_front()
            .expect("发生意外额外请求");
        Box::pin(async move {
            match step {
                Step::Batch(batch) => Ok(batch),
                Step::Error => Err(CommandError::provider(
                    "PROVIDER_REQUEST_FAILED",
                    "模型连接失败",
                )),
                Step::Cancel(control) => {
                    control.cancel();
                    std::future::pending().await
                }
            }
        })
    }
}

struct Dictionary {
    fail: bool,
}
impl DictionaryLookup for Dictionary {
    /// 隔离词典故障以验证兄弟卡片调用仍能成功。
    fn lookup<'a>(&'a self, _: &'a str) -> PortFuture<'a, Option<DictionaryEntry>> {
        Box::pin(async move {
            if self.fail {
                Err(CommandError::new("DICTIONARY_ERROR", "不可用"))
            } else {
                Ok(None)
            }
        })
    }
}

/// 默认输入请求五张但材料仅有一个可用摘录。
fn input() -> GenerationInput {
    GenerationInput {
        type_id: "qa".to_string(),
        study_mode_id: "self-review".to_string(),
        note_title: "测试".to_string(),
        source_text: "学习材料".to_string(),
        images: vec![],
        requested_count: 5,
        context: None,
    }
}

/// 构造合法问答参数，便于只改变失败条件。
fn arguments(question: &str) -> Value {
    json!({"schema_version":1,"type_id":"qa","source":"学习材料","fields":{"front":question,"back":"答案","detail":""}})
}

/// 工具 ID 固定但不进入日志或最终卡片身份。
fn call(name: &str, value: Value) -> ToolCallResult {
    ToolCallResult {
        id: name.to_string(),
        item_id: None,
        name: name.to_string(),
        arguments: ToolArguments::Valid(value),
    }
}

/// 一次模型响应支持同时包含成功和失败调用。
fn batch(calls: Vec<ToolCallResult>) -> Step {
    Step::Batch(ToolCallBatch {
        calls,
        continuation_items: vec![],
    })
}

/// 清单是落卡的前置条件；测试统一引用材料里的唯一摘录。
fn plan(keywords: &[&str]) -> ToolCallResult {
    let items = keywords
        .iter()
        .map(|keyword| json!({"source":"学习材料","keyword":keyword}))
        .collect::<Vec<_>>();
    call("plan_cards", json!({ "items": items }))
}

/// 从给定剧本构造执行器端口，记录请求数以验证停止边界。
fn scripted_model(steps: Vec<Step>) -> Model {
    Model {
        steps: Mutex::new(steps.into()),
        calls: AtomicUsize::new(0),
    }
}

/// 执行经过领域输入校验的会话。
async fn run(
    model: &Model,
    input: &GenerationInput,
    control: &GenerationControl,
) -> Result<GenerationResult, CommandError> {
    execute_generation(
        model,
        &Dictionary { fail: false },
        input,
        GenerationSession::new(input).unwrap(),
        control,
    )
    .await
}

#[tokio::test]
/// 材料不足可以少生成，也允许零张并给出原因。
async fn permits_fewer_cards_and_zero_cards() {
    for produce_card in [false, true] {
        let mut calls = vec![call(
            "finish_generation",
            json!({"reason":"材料中没有更多独立考点"}),
        )];
        if produce_card {
            calls.push(plan(&["考点"]));
            calls.push(call("emit_card", arguments("问题")));
        }
        let model = scripted_model(vec![batch(calls)]);
        let result = run(
            &model,
            &input(),
            &GenerationControl::new("test".to_string()),
        )
        .await
        .unwrap();
        assert_eq!(result.cards.len(), usize::from(produce_card));
        assert_eq!(result.warnings, ["材料中没有更多独立考点"]);
    }
}

#[tokio::test]
/// 后续连接失败不会丢失已经通过校验的草稿。
async fn preserves_partial_cards_on_later_failure() {
    let model = scripted_model(vec![
        batch(vec![plan(&["考点"]), call("emit_card", arguments("问题"))]),
        Step::Error,
    ]);
    let result = run(
        &model,
        &input(),
        &GenerationControl::new("test".to_string()),
    )
    .await
    .unwrap();
    assert_eq!(result.cards.len(), 1);
    assert!(result.warnings[0].contains("请求失败"));
    let model = scripted_model(vec![Step::Error]);
    assert_eq!(
        run(
            &model,
            &input(),
            &GenerationControl::new("test".to_string())
        )
        .await
        .unwrap_err()
        .code,
        "PROVIDER_REQUEST_FAILED"
    );
}

#[tokio::test]
/// 请求等待中停止以及启动立即停止均及时返回，部分草稿完整保留。
async fn cancels_pending_request_and_preserves_partial() {
    let control = GenerationControl::new("test".to_string());
    let model = scripted_model(vec![
        batch(vec![plan(&["考点"]), call("emit_card", arguments("问题"))]),
        Step::Cancel(control.clone()),
    ]);
    let result = tokio::time::timeout(Duration::from_secs(1), run(&model, &input(), &control))
        .await
        .unwrap()
        .unwrap();
    assert_eq!(result.cards.len(), 1);
    assert!(result.warnings[0].contains("停止"));
    let control = GenerationControl::new("immediate".to_string());
    control.cancel();
    let model = scripted_model(vec![]);
    assert!(run(&model, &input(), &control)
        .await
        .unwrap()
        .cards
        .is_empty());
    assert_eq!(model.calls.load(Ordering::SeqCst), 0);
}

#[tokio::test]
/// 每轮都有合法卡片时持续推进，达到请求数量后自然结束。
async fn progressing_rounds_continue_until_requested_count() {
    let mut steps = vec![batch(vec![
        plan(&["考点0", "考点1", "考点2", "考点3", "考点4"]),
        call("emit_card", json!({})),
        call("emit_card", arguments("问题0")),
    ])];
    steps.extend((1..5).map(|index| {
        batch(vec![
            call("emit_card", json!({})),
            call("emit_card", arguments(&format!("问题{index}"))),
        ])
    }));
    let model = scripted_model(steps);
    let result = run(
        &model,
        &input(),
        &GenerationControl::new("test".to_string()),
    )
    .await
    .unwrap();
    assert_eq!(result.cards.len(), 5);
    assert_eq!(model.calls.load(Ordering::SeqCst), 5);
}

#[tokio::test]
/// 重复调用不再硬停：停滞只写进阶段提示，草稿保留，由值守者决定是否停止。
async fn stagnant_rounds_warn_without_terminating() {
    let control = GenerationControl::new("test".to_string());
    let cancel = control.clone();
    let model = scripted_model(
        (0..4)
            .map(|_| batch(vec![call("emit_card", arguments("问题"))]))
            .chain(std::iter::once(Step::Cancel(cancel)))
            .collect::<Vec<_>>()
            .into_iter()
            .enumerate()
            .map(|(index, step)| match (index, step) {
                (0, Step::Batch(mut batch)) => {
                    batch.calls.insert(0, plan(&["考点"]));
                    Step::Batch(batch)
                }
                (_, step) => step,
            })
            .collect(),
    );
    let result = run(&model, &input(), &control).await.unwrap();
    assert_eq!(result.cards.len(), 1);
    assert_eq!(model.calls.load(Ordering::SeqCst), 5);
    assert!(result.warnings[0].contains("停止"));
}

#[tokio::test]
/// 没提交清单就不允许落卡；先 plan 才接受卡片。
async fn emit_requires_a_plan() {
    let model = scripted_model(vec![
        batch(vec![call("emit_card", arguments("问题"))]),
        batch(vec![plan(&["考点"]), call("emit_card", arguments("问题"))]),
        batch(vec![call(
            "finish_generation",
            json!({"reason":"清单已完成"}),
        )]),
    ]);
    let result = run(
        &model,
        &input(),
        &GenerationControl::new("test".to_string()),
    )
    .await
    .unwrap();
    assert_eq!(result.cards.len(), 1);
    assert_eq!(model.calls.load(Ordering::SeqCst), 3);
}

#[tokio::test]
/// 清单未落地完不允许结束；全部落地后 finish_generation 才能收尾。
async fn finish_waits_for_the_plan_to_be_emitted() {
    let model = scripted_model(vec![
        batch(vec![
            plan(&["考点"]),
            call("finish_generation", json!({"reason":"提前结束"})),
        ]),
        batch(vec![call("emit_card", arguments("问题"))]),
        batch(vec![call(
            "finish_generation",
            json!({"reason":"清单已完成"}),
        )]),
    ]);
    let result = run(
        &model,
        &input(),
        &GenerationControl::new("test".to_string()),
    )
    .await
    .unwrap();
    assert_eq!(result.cards.len(), 1);
    assert_eq!(result.warnings, ["清单已完成"]);
    assert_eq!(model.calls.load(Ordering::SeqCst), 3);
}

#[tokio::test]
/// 历史再大也不裁剪、不硬停；只有模型主动结束才停止。
async fn large_history_is_not_truncated_or_stopped() {
    let model = scripted_model(vec![
        Step::Batch(ToolCallBatch {
            calls: vec![plan(&["考点"]), call("emit_card", arguments("问题"))],
            continuation_items: vec![json!({"encrypted_content":"x".repeat(1024 * 1024)})],
        }),
        batch(vec![call(
            "finish_generation",
            json!({"reason":"材料已用尽"}),
        )]),
    ]);
    let result = run(
        &model,
        &input(),
        &GenerationControl::new("test".to_string()),
    )
    .await
    .unwrap();
    assert_eq!(result.cards.len(), 1);
    assert_eq!(model.calls.load(Ordering::SeqCst), 2);
    assert_eq!(result.warnings, ["材料已用尽"]);
}

#[tokio::test]
/// 单工具词典故障不阻止同轮有效草稿达到数量上限。
async fn dictionary_failure_is_isolated() {
    let mut input = input();
    input.type_id = "vocabulary".to_string();
    input.study_mode_id = "dictation".to_string();
    input.requested_count = 1;
    let model = scripted_model(vec![batch(vec![
        plan(&["speak"]),
        call("lookup_words", json!({"words":["speak"]})),
        call(
            "emit_card",
            json!({"schema_version":1,"type_id":"vocabulary","source":"学习材料","fields":{"front":"v. 说","back":"speak","detail":"","example":"","aliases":""}}),
        ),
    ])]);
    let result = execute_generation(
        &model,
        &Dictionary { fail: true },
        &input,
        GenerationSession::new(&input).unwrap(),
        &GenerationControl::new("test".to_string()),
    )
    .await
    .unwrap();
    assert_eq!(result.cards.len(), 1);
}

#[tokio::test]
/// 同轮成功查询必须先给模型看到结果，下一轮才接受卡片。
async fn successful_lookup_defers_same_batch_card() {
    let mut input = input();
    input.type_id = "vocabulary".to_string();
    input.study_mode_id = "dictation".to_string();
    input.requested_count = 1;
    let card = json!({"schema_version":1,"type_id":"vocabulary","source":"学习材料","fields":{"front":"v. 说","back":"speak","detail":"","example":"","aliases":""}});
    let model = scripted_model(vec![
        batch(vec![
            plan(&["speak"]),
            call("emit_card", card.clone()),
            call("lookup_words", json!({"words":["speak"]})),
        ]),
        batch(vec![call("emit_card", card)]),
    ]);
    let result = run(&model, &input, &GenerationControl::new("test".to_string()))
        .await
        .unwrap();
    assert_eq!(result.cards.len(), 1);
    assert_eq!(model.calls.load(Ordering::SeqCst), 2);
}
