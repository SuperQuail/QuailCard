use super::*;

const ARTIFACT_REFS: [&str; 4] = [
    "task:done",
    "change:saved",
    "subagent:child",
    "legacy-result",
];

/// 通过存储端口准备真实笔记，使完成证据必须经过 read_note 的 hash 校验。
fn completion_note(files: &AgentFiles, session: &AgentSession) {
    files
        .change(&session.id, "completion.md", "已经核实的解释", None)
        .unwrap();
}

/// 四项验收必须逐项覆盖，不能用一条合法收据掩盖遗漏的条件。
fn completion_create_four() -> Response {
    fixed(reply(
        "create_goal",
        json!({"objective":"核实四项解释", "acceptanceCriteria":["甲","乙","丙","丁"]}),
    ))
}

/// 只引用目录里真实成功的读笔记收据，不硬编码运行时生成的调用标识。
fn completion_evidence(state: &Value) -> Value {
    let receipt = state["receipts"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|entry| entry["tools"].as_array())
        .flatten()
        .find(|entry| entry["successful"] == true && entry["path"] == "completion.md")
        .expect("必须有真实 read_note 收据");
    let evidence: Vec<_> = state["goal"]["acceptanceCriteria"]
        .as_array()
        .unwrap()
        .iter()
        .enumerate()
        .map(|(index, _)| {
            json!({"criterionIndex":index,"goalRevision":state["goal"]["revision"],
                "sourceVersion":state["sourceVersion"],"receiptRef":receipt["receiptRef"]})
        })
        .collect();
    json!(evidence)
}

/// 拒绝后同一 Goal 必须仍是初始修订，且不能留下部分 evidence 或 blocker。
fn completion_assert_active(state: &Value) {
    assert_eq!(state["goal"]["phase"], "active");
    assert_eq!(state["goal"]["revision"], 1);
    assert_eq!(state["goal"]["roundsStarted"], 0);
    assert_eq!(state["goal"]["evidence"], json!([]));
    assert!(state["goal"]["blocker"].is_null());
}

/// 统一提交当前快照，失败重试不得偷偷创建新目标或修改旧目标规格。
fn completion_finish(poison: Option<(usize, &'static str)>) -> Response {
    Box::new(move |history, _| {
        let state = snapshot(history, "goal");
        completion_assert_active(&state);
        let mut evidence = completion_evidence(&state);
        if let Some((index, reference)) = poison {
            evidence[index]["receiptRef"] = json!(reference);
        }
        reply(
            "update_goal",
            json!({"goalId":state["goal"]["id"],"revision":state["goal"]["revision"],
                "action":"complete","evidence":evidence}),
        )
    })
}

/// 检查真实工具错误并重新读取状态，确保模型能按可定位消息修正后继续。
fn completion_rejected(code: &'static str, location: &'static str) -> Response {
    Box::new(move |history, _| {
        let last = history.iter().rev().find(|m| m["role"] == "tool").unwrap();
        let result: Value = serde_json::from_str(last["content"].as_str().unwrap()).unwrap();
        assert_eq!(result["ok"], false);
        assert_eq!(result["error"]["code"], code);
        assert!(
            result["error"]["message"]
                .as_str()
                .unwrap()
                .contains(location),
            "错误缺少位置 {location}: {result}"
        );
        reply("get_goal", json!({}))
    })
}

/// 计划整表替换始终采用宿主修订，产物引用无需伪造成完成收据。
fn completion_plan(steps: Value) -> Response {
    Box::new(move |history, _| {
        let state = snapshot(history, "planRevision");
        reply(
            "update_plan",
            json!({"planRevision":state["planRevision"],"steps":steps}),
        )
    })
}

/// 旧格式 resultRefs 原样保留，缺 required 字段仍使用保守的必需语义。
fn completion_step(status: &str) -> Value {
    json!({"id":"finish","text":"核实并收尾","status":status,"resultRefs":ARTIFACT_REFS})
}

/// 四类产物引用跨会话重载保持兼容，完成只需真实收据逐项覆盖四个验收条件。
#[tokio::test]
async fn completion_artifact_refs_survive_reload_with_real_tool_evidence() {
    let root = TempDir::new();
    let files = AgentFiles::new(root.path()).unwrap();
    let mut session = files.create_session().unwrap();
    completion_note(&files, &session);
    run_script(
        &files,
        &mut session,
        "完成四项核实，先等我确认范围",
        vec![
            completion_create_four(),
            completion_plan(json!([completion_step("completed")])),
            wait(),
        ],
    )
    .await
    .unwrap();
    let mut restored = files.session(&session.id).unwrap();
    let goal_id = restored.goal.as_ref().unwrap().id.clone();
    let plan = restored.plan.clone();
    assert_eq!(plan.steps[0].result_refs, ARTIFACT_REFS);
    assert!(plan.steps[0].required);
    run_script(
        &files,
        &mut restored,
        "请核实笔记并完成原目标",
        vec![
            fixed(reply("read_note", json!({"path":"completion.md"}))),
            fixed(reply("get_goal", json!({}))),
            completion_finish(None),
        ],
    )
    .await
    .unwrap();
    let stored = files.session(&session.id).unwrap();
    assert_eq!(stored.plan, plan);
    let goal = stored.goal.unwrap();
    assert_eq!(goal.id, goal_id);
    assert_eq!(goal.phase, GoalPhase::Complete);
    assert_eq!(goal.evidence.len(), 4);
    assert!(goal
        .evidence
        .iter()
        .all(|e| e.receipt_ref.starts_with("tool:")));
}

/// 产物引用放进 evidence 仍逐次拒绝，不能污染状态，也不能阻止有效证据恢复。
#[tokio::test]
async fn completion_artifact_evidence_rejected_without_mutation_then_recovers() {
    let root = TempDir::new();
    let files = AgentFiles::new(root.path()).unwrap();
    let mut session = files.create_session().unwrap();
    completion_note(&files, &session);
    let mut script = vec![
        completion_create_four(),
        completion_plan(json!([completion_step("completed")])),
        fixed(reply("read_note", json!({"path":"completion.md"}))),
        fixed(reply("get_goal", json!({}))),
    ];
    for reference in ARTIFACT_REFS {
        script.push(completion_finish(Some((2, reference))));
        script.push(completion_rejected("GOAL_INCOMPLETE", "evidence[2]"));
    }
    script.push(completion_finish(None));
    run_script(&files, &mut session, "完成四项核实", script)
        .await
        .unwrap();
    let stored = files.session(&session.id).unwrap();
    assert_eq!(stored.plan.revision, 1);
    assert_eq!(stored.plan.steps[0].result_refs, ARTIFACT_REFS);
    let goal = stored.goal.unwrap();
    assert_eq!(goal.phase, GoalPhase::Complete);
    assert_eq!(goal.revision, 2);
    assert_eq!(goal.evidence.len(), 4);
    assert_eq!(
        results(&session)
            .iter()
            .filter(|r| r["ok"] == false)
            .count(),
        4
    );
}

/// 收尾步骤未完成时明确指出零基索引，计划修正后同一 Goal 才允许完成。
#[tokio::test]
async fn completion_in_progress_step_reports_zero_based_index_then_recovers() {
    let root = TempDir::new();
    let files = AgentFiles::new(root.path()).unwrap();
    let mut session = files.create_session().unwrap();
    completion_note(&files, &session);
    run_script(
        &files,
        &mut session,
        "核实解释并完成收尾",
        vec![
            create(),
            completion_plan(json!([completion_step("in_progress")])),
            fixed(reply("read_note", json!({"path":"completion.md"}))),
            fixed(reply("get_goal", json!({}))),
            completion_finish(None),
            completion_rejected("GOAL_INCOMPLETE", "steps[0]"),
            completion_plan(json!([completion_step("completed")])),
            fixed(reply("get_goal", json!({}))),
            completion_finish(None),
        ],
    )
    .await
    .unwrap();
    let states = results(&session);
    let original_id = &states[0]["result"]["goal"]["id"];
    let goal = session.goal.as_ref().unwrap();
    assert_eq!(json!(goal.id), *original_id);
    assert_eq!(goal.phase, GoalPhase::Complete);
    assert_eq!(goal.revision, 2);
    assert_eq!(session.plan.revision, 2);
    assert!(session.plan.steps[0].required);
}

/// 显式可选且取消的步骤允许保留产物引用，不妨碍已完成的必需步骤验收。
#[tokio::test]
async fn completion_optional_cancelled_step_and_refs_do_not_block() {
    let root = TempDir::new();
    let files = AgentFiles::new(root.path()).unwrap();
    let mut session = files.create_session().unwrap();
    completion_note(&files, &session);
    run_script(
        &files,
        &mut session,
        "完成解释，可取消额外排版",
        vec![
            create(),
            completion_plan(json!([
                {"id":"answer","text":"解释","status":"completed"},
                {"id":"optional","text":"额外排版","status":"cancelled",
                    "required":false,"resultRefs":ARTIFACT_REFS}
            ])),
            fixed(reply("read_note", json!({"path":"completion.md"}))),
            fixed(reply("get_goal", json!({}))),
            completion_finish(None),
        ],
    )
    .await
    .unwrap();
    let stored = files.session(&session.id).unwrap();
    assert_eq!(stored.goal.unwrap().phase, GoalPhase::Complete);
    assert!(stored.plan.steps[0].required);
    assert!(!stored.plan.steps[1].required);
    assert_eq!(stored.plan.steps[1].status, PlanStatus::Cancelled);
    assert_eq!(stored.plan.steps[1].result_refs, ARTIFACT_REFS);
}

/// 取消状态不能隐式变成可选；必须明确 required=false 后才解除完成门槛。
#[tokio::test]
async fn completion_cancelled_step_defaults_required_until_explicitly_optional() {
    let root = TempDir::new();
    let files = AgentFiles::new(root.path()).unwrap();
    let mut session = files.create_session().unwrap();
    completion_note(&files, &session);
    let mut optional = completion_step("cancelled");
    optional["required"] = json!(false);
    run_script(
        &files,
        &mut session,
        "核实后取消非必要收尾",
        vec![
            create(),
            completion_plan(json!([completion_step("cancelled")])),
            fixed(reply("read_note", json!({"path":"completion.md"}))),
            fixed(reply("get_goal", json!({}))),
            completion_finish(None),
            completion_rejected("GOAL_INCOMPLETE", "steps[0]"),
            completion_plan(json!([optional])),
            fixed(reply("get_goal", json!({}))),
            completion_finish(None),
        ],
    )
    .await
    .unwrap();
    assert_eq!(session.goal.as_ref().unwrap().phase, GoalPhase::Complete);
    assert!(!session.plan.steps[0].required);
    assert_eq!(session.plan.steps[0].result_refs, ARTIFACT_REFS);
}

#[path = "agent_completion_validation_tests.rs"]
mod validation_tests;
