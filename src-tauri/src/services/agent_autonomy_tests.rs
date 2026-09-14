use super::*;
use crate::agent_autonomy_models::{GoalPhase, PlanStatus};
use crate::agent_models::AgentRun;

type Response = Box<dyn FnOnce(&[Value], &[ToolDefinition]) -> AgentModelReply + Send>;
struct Script(Mutex<VecDeque<Response>>);
impl AgentModel for Script {
    /// 动态脚本只读取宿主提供的历史，不猜测 Goal 修订或收据 UUID。
    fn call<'a>(
        &'a self,
        _: &'a str,
        history: &'a [Value],
        definitions: &'a [ToolDefinition],
        delta: &'a (dyn Fn(&str) + Send + Sync),
        _: &'a (dyn Fn(&str) + Send + Sync),
    ) -> AgentFuture<'a, AgentModelReply> {
        Box::pin(async move {
            let next = self
                .0
                .lock()
                .unwrap()
                .pop_front()
                .expect("出现未预期的模型续调");
            let result = next(history, definitions);
            delta(&result.text);
            Ok(result)
        })
    }
}

/// 固定响应与动态响应共用脚本，响应耗尽时测试立即失败而非吞错。
fn fixed(reply: AgentModelReply) -> Response {
    Box::new(move |_, _| reply)
}

/// 普通回复是轮次结束，不预先假定它会终结 Goal。
fn say(text: &str) -> Response {
    fixed(AgentModelReply {
        text: text.into(),
        calls: vec![],
        replay: json!({"role":"assistant","content":text}),
    })
}

/// 新工具不接受额度参数；脚本必须明确完成或等待用户。
fn create() -> Response {
    fixed(reply(
        "create_goal",
        json!({"objective":"解释两点", "acceptanceCriteria":["给出解释"]}),
    ))
}

/// 用户等待场景共用同一个明确问题，避免脚本误返回普通文本。
fn wait() -> Response {
    fixed(reply("wait_for_user", json!({"question":"请选择范围"})))
}

/// 从最近成功工具结果取当前快照，覆盖真实工具 JSON 信封而非直接读会话。
fn snapshot(history: &[Value], key: &str) -> Value {
    let mut tools = history.iter().rev().filter(|m| m["role"] == "tool");
    tools
        .find_map(|m| {
            let value: Value = serde_json::from_str(m["content"].as_str()?).ok()?;
            (value["ok"] == true && value["result"].get(key).is_some())
                .then(|| value["result"].clone())
        })
        .expect("缺少成功的状态读取结果")
}

/// 与原 fixture 不同：保留执行结果及等待快照，并且绝不批准被测禁止写入。
async fn run_script(
    files: &AgentFiles,
    session: &mut AgentSession,
    content: &str,
    responses: Vec<Response>,
) -> Result<AgentRun, CommandError> {
    let input = AgentInput {
        session_id: session.id.clone(),
        request_id: uuid::Uuid::now_v7().to_string(),
        content: content.into(),
        provider_id: "test".into(),
        selected_paths: vec![],
        images: vec![],
    };
    let tasks = AgentTasks::default();
    let (control, _) = tasks
        .register("main", "root", &input.request_id, &session.id)
        .unwrap();
    let model = Script(Mutex::new(responses.into()));
    let cards = FakeCards::default();
    let result = tokio::time::timeout(
        Duration::from_secs(3),
        execute(
            AgentPorts {
                model: &model,
                repository: files,
                learning: &FakeLearning,
                video: &FakeVideo,
                dictionary: &FakeDictionary,
                cards: &cards,
            },
            session,
            &input,
            &control,
        ),
    )
    .await
    .expect("执行不应等待写入或无限续调");
    assert!(
        model.0.lock().unwrap().is_empty(),
        "模型提前结束：{result:?}"
    );
    result.map(|()| control.snapshot())
}

/// 从完整持久交换提取回执，验证拒绝发生在实际执行入口。
fn results(session: &AgentSession) -> Vec<Value> {
    session
        .messages
        .iter()
        .filter(|m| m.kind == "exchange")
        .flat_map(|m| m.data["results"].as_array().unwrap())
        .map(|r| serde_json::from_str(r["content"].as_str().unwrap()).unwrap())
        .collect()
}

/// 新建目标不设置轮次上限，普通回复后继续直到明确等待用户。
#[tokio::test]
async fn ordinary_finish_continues_until_explicit_wait() {
    let root = TempDir::new();
    let files = AgentFiles::new(root.path()).unwrap();
    let mut session = files.create_session().unwrap();
    run_script(
        &files,
        &mut session,
        "请完整解释",
        vec![
            create(),
            say("第一段"),
            say("第二段"),
            say("第三段"),
            wait(),
        ],
    )
    .await
    .unwrap();
    let stored = files.session(&session.id).unwrap();
    let goal = stored.goal.unwrap();
    assert_eq!(goal.rounds_started, 3);
    assert_eq!(goal.phase, GoalPhase::Active);
    assert!(goal.blocker.is_none());
    assert_eq!(
        stored
            .messages
            .iter()
            .filter(|m| m.kind == "goal_round")
            .count(),
        3
    );
}

/// 完成使用真实文本收据，同批业务调用被拒绝，终态后不再请求总结。
#[tokio::test]
async fn complete_rejects_later_business_tools() {
    let root = TempDir::new();
    let files = AgentFiles::new(root.path()).unwrap();
    let mut session = files.create_session().unwrap();
    let finish: Response = Box::new(|history, _| {
        let state = snapshot(history, "goal");
        let receipt = state["receipts"]
            .as_array()
            .unwrap()
            .iter()
            .find(|r| {
                r["receiptRef"]
                    .as_str()
                    .is_some_and(|s| s.starts_with("message:"))
            })
            .unwrap();
        multi_reply(vec![
            AgentCall {
                id: "complete".into(),
                name: "update_goal".into(),
                arguments: json!({
                    "goalId":state["goal"]["id"],"revision":state["goal"]["revision"],"action":"complete",
                    "evidence":[{"criterionIndex":0,"goalRevision":state["goal"]["revision"],
                    "sourceVersion":state["sourceVersion"],"receiptRef":receipt["receiptRef"]}]
                }),
            },
            AgentCall {
                id: "forbidden".into(),
                name: "remember".into(),
                arguments: json!({"content":"禁止落地"}),
            },
        ])
    });
    let result = run_script(
        &files,
        &mut session,
        "请解释",
        vec![
            create(),
            say("解释已给出"),
            fixed(reply("get_goal", json!({}))),
            finish,
        ],
    )
    .await;
    assert!(result.is_ok());
    assert_eq!(session.goal.as_ref().unwrap().phase, GoalPhase::Complete);
    assert_eq!(session.goal.as_ref().unwrap().rounds_started, 1);
    assert!(results(&session)
        .iter()
        .any(|r| r["error"]["code"] == "AGENT_WAITING"));
    assert!(!session.messages.iter().any(|m| m.kind == "memory"));
    assert!(!files.memory().unwrap().content.contains("禁止落地"));
}

/// 需要用户回答会马上挂起，不能在后台继续消耗 Goal 轮次。
#[tokio::test]
async fn waiting_user_suspends_without_another_model_call() {
    let root = TempDir::new();
    let files = AgentFiles::new(root.path()).unwrap();
    let mut session = files.create_session().unwrap();
    let run = run_script(&files, &mut session, "请解释", vec![create(), wait()])
        .await
        .unwrap();
    assert_eq!(run.waiting_reason.as_deref(), Some("waitingUser"));
    let goal = files.session(&session.id).unwrap().goal.unwrap();
    assert_eq!(goal.phase, GoalPhase::Active);
    assert_eq!(goal.rounds_started, 0);
}

/// 重新打开保存的 active Goal 不授予自动许可，必须当前用户明确 resume。
#[tokio::test]
async fn restored_goal_requires_explicit_resume() {
    let root = TempDir::new();
    let files = AgentFiles::new(root.path()).unwrap();
    let mut session = files.create_session().unwrap();
    run_script(&files, &mut session, "请解释", vec![create(), wait()])
        .await
        .unwrap();
    let mut restored = files.session(&session.id).unwrap();
    // 模拟旧会话已经用完轮次额度；查看不武装，明确恢复后仍须继续。
    let legacy_goal = restored.goal.as_mut().unwrap();
    legacy_goal.max_goal_rounds = 1;
    legacy_goal.rounds_started = 1;
    files.save_session(&restored).unwrap();
    run_script(&files, &mut restored, "仅查看进展", vec![say("尚待选择")])
        .await
        .unwrap();
    assert_eq!(restored.goal.as_ref().unwrap().rounds_started, 1);
    let resume: Response = Box::new(|history, _| {
        let state = snapshot(history, "goal");
        assert_eq!(state["armed"], false);
        reply(
            "update_goal",
            json!({"goalId":state["goal"]["id"],"revision":state["goal"]["revision"],"action":"resume"}),
        )
    });
    run_script(
        &files,
        &mut restored,
        "请继续完成",
        vec![
            fixed(reply("get_goal", json!({}))),
            resume,
            say("继续"),
            say("继续检查"),
            say("仍未完成"),
            wait(),
        ],
    )
    .await
    .unwrap();
    assert_eq!(restored.goal.as_ref().unwrap().rounds_started, 4);
    assert_eq!(restored.goal.as_ref().unwrap().phase, GoalPhase::Active);
}

/// 子会话即使伪造未声明的业务工具也被入口拒绝，不能仅靠提示词或工具隐藏。
#[tokio::test]
async fn child_cannot_create_goal_or_escape_business_allowlist() {
    let root = TempDir::new();
    let files = AgentFiles::new(root.path()).unwrap();
    let mut session = files.create_session().unwrap();
    session.parent_session_id = Some("parent".into());
    session.delegation_depth = 1;
    let attack: Response = Box::new(|_, definitions| {
        for name in ["read_note", "lookup_dictionary"] {
            assert!(definitions.iter().any(|d| d.name == name));
        }
        for name in [
            "remember",
            "video_note",
            "delete_card",
            "generate_cards",
            "create_note",
        ] {
            assert!(!definitions.iter().any(|d| d.name == name));
        }
        let mut calls = reply(
            "create_goal",
            json!({"objective":"越权", "acceptanceCriteria":["越权"]}),
        )
        .calls;
        calls.extend(
            [
                "remember",
                "video_note",
                "delete_card",
                "generate_cards",
                "create_note",
            ]
            .iter()
            .enumerate()
            .map(|(i, name)| AgentCall {
                id: format!("attack-{i}"),
                name: (*name).into(),
                arguments: json!({}),
            }),
        );
        multi_reply(calls)
    });
    run_script(
        &files,
        &mut session,
        "委派分析",
        vec![attack, say("向父级报告")],
    )
    .await
    .unwrap();
    assert!(session.goal.is_none());
    let receipts = results(&session);
    assert_eq!(receipts.len(), 6);
    assert!(receipts.iter().all(|r| r["ok"] == false));
    assert_eq!(receipts[0]["error"]["code"], "AGENT_AUTHORITY_DENIED");
    assert!(!session
        .messages
        .iter()
        .any(|m| ["change", "memory", "drafts"].contains(&m.kind.as_str())));
}

/// get_plan 修订驱动整表更新，避免测试以硬编码版本掩盖 CAS 问题。
fn update_plan(status: &'static str) -> Response {
    Box::new(move |history, _| {
        let state = snapshot(history, "planRevision");
        reply(
            "update_plan",
            json!({"planRevision":state["planRevision"],"steps":[
            {"id":"a","text":"独立任务甲","status":status},{"id":"b","text":"独立任务乙","status":status}]}),
        )
    })
}

/// 并行计划落盘后下一轮整表替换只能追加历史，旧 Fork 完整轮次前缀逐字不变。
#[tokio::test]
async fn parallel_plan_replacement_preserves_completed_fork_prefix() {
    let root = TempDir::new();
    let files = AgentFiles::new(root.path()).unwrap();
    let mut session = files.create_session().unwrap();
    run_script(
        &files,
        &mut session,
        "并行分析",
        vec![
            fixed(reply("get_plan", json!({}))),
            update_plan("in_progress"),
            say("并行任务进行中"),
        ],
    )
    .await
    .unwrap();
    let boundary = session.completed_message_count;
    let prefix = serde_json::to_value(&session.messages[..boundary]).unwrap();
    let stored = files.session(&session.id).unwrap();
    assert_eq!(stored.plan.steps.len(), 2);
    assert!(stored
        .plan
        .steps
        .iter()
        .all(|s| s.status == PlanStatus::InProgress));
    run_script(
        &files,
        &mut session,
        "更新结果",
        vec![
            fixed(reply("get_plan", json!({}))),
            update_plan("completed"),
            say("任务结束"),
        ],
    )
    .await
    .unwrap();
    assert_eq!(
        serde_json::to_value(&session.messages[..boundary]).unwrap(),
        prefix
    );
    let stored = files.session(&session.id).unwrap();
    assert_eq!(stored.plan.revision, 2);
    assert_eq!(
        stored.messages.iter().filter(|m| m.kind == "plan").count(),
        2
    );
    assert!(stored
        .plan
        .steps
        .iter()
        .all(|s| s.status == PlanStatus::Completed));
    assert!(stored.completed_message_count > boundary);
}

#[path = "agent_completion_tests.rs"]
mod completion_tests;
