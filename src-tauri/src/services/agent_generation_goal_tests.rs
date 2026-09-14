use super::*;

/// 模型正文与 get_goal 同轮出现，提供真实文本收据而不触发普通结束续轮。
fn read_goal_with_text() -> Response {
    let mut response = reply("get_goal", json!({}));
    response.text = "已解释材料，但卡片仍须用户采纳".into();
    response.replay["content"] = json!(response.text);
    fixed(response)
}

/// 使用实时快照生成完成请求，避免错误的修订或缺失证据掩盖草稿守卫回归。
fn completion(state: &Value) -> AgentCall {
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
    AgentCall {
        id: "finish-goal".into(),
        name: "update_goal".into(),
        arguments: json!({
            "goalId":state["goal"]["id"],"revision":state["goal"]["revision"],"action":"complete",
            "evidence":[{"criterionIndex":0,"goalRevision":state["goal"]["revision"],
            "sourceVersion":state["sourceVersion"],"receiptRef":receipt["receiptRef"]}]
        }),
    }
}

/// QA 卡走真实生成 schema 与领域管线，不用手工草稿模拟生成中的行为。
fn card_calls() -> Vec<AgentCall> {
    vec![
        AgentCall {
            id: "plan-card".into(),
            name: "plan_cards".into(),
            arguments: json!({"items":[{"source":"学习材料","keyword":"考点"}]}),
        },
        AgentCall {
            id: "emit-card".into(),
            name: "emit_card".into(),
            arguments: json!({"schema_version":1,"type_id":"qa","source":"学习材料",
                "fields":{"front":"问题","back":"答案","detail":""}}),
        },
        AgentCall {
            id: "finish-cards".into(),
            name: "finish_generation".into(),
            arguments: json!({"reason":"材料已用尽"}),
        },
    ]
}

/// 启动模式只通过真实 Agent 工具，后续测试必须观察到生成工具声明。
fn start_cards() -> Response {
    fixed(reply("generate_cards", json!({"path":"a.md","kind":"qa"})))
}

/// 在隔离知识库准备真实可读笔记，fixture 生成资料与已有 QA 测试一致。
fn generation_fixture() -> (TempDir, AgentFiles, AgentSession) {
    let root = TempDir::new();
    let files = AgentFiles::new(root.path()).unwrap();
    files
        .change(&uuid::Uuid::now_v7().to_string(), "a.md", "学习材料", None)
        .unwrap();
    let session = files.create_session().unwrap();
    (root, files, session)
}

/// 草稿落盘需要立即挂起，不能再向模型发起总结或自动续轮请求。
fn assert_waiting_drafts(files: &AgentFiles, session: &AgentSession, run: &AgentRun) {
    assert_eq!(run.waiting_reason.as_deref(), Some("waitingUser"));
    let stored = files.session(&session.id).unwrap();
    let goal = stored.goal.as_ref().unwrap();
    assert_eq!(goal.phase, GoalPhase::Active);
    assert_eq!(goal.rounds_started, 0);
    let drafts = stored.messages.iter().find(|m| m.kind == "drafts").unwrap();
    assert_eq!(drafts.data["cards"].as_array().unwrap().len(), 1);
    assert_eq!(drafts.data["goalId"], goal.id);
    assert_eq!(drafts.data["goalRevision"], goal.revision);
    assert_eq!(drafts.data["adoptionResolved"], false);
    assert_eq!(stored.completed_message_count, stored.messages.len());
}

/// 即便验收文本完整，活动拆卡模式仍拒绝完成，不可借文本收据提前结束生成。
#[tokio::test]
async fn active_generation_rejects_complete() {
    let (_root, files, mut session) = generation_fixture();
    let attempt: Response = Box::new(|history, definitions| {
        assert!(definitions
            .iter()
            .any(|tool| tool.name == "finish_generation"));
        let state = snapshot(history, "goal");
        assert_eq!(state["pendingDrafts"], false);
        multi_reply(vec![completion(&state)])
    });
    run_script(
        &files,
        &mut session,
        "解释并生成卡片",
        vec![
            create(),
            start_cards(),
            read_goal_with_text(),
            attempt,
            wait(),
        ],
    )
    .await
    .unwrap();
    assert_eq!(session.goal.as_ref().unwrap().phase, GoalPhase::Active);
    assert!(results(&session)
        .iter()
        .any(|r| r["error"]["code"] == "GOAL_INCOMPLETE"));
    assert!(!session.messages.iter().any(|m| m.kind == "drafts"));
}

/// 同批先处理生成工具也不能让 complete 越过待采纳阶段，必须落盘并挂起。
#[tokio::test]
async fn finish_generation_and_complete_in_one_batch_is_rejected() {
    let (_root, files, mut session) = generation_fixture();
    let batch: Response = Box::new(|history, definitions| {
        assert!(definitions.iter().any(|tool| tool.name == "emit_card"));
        let mut calls = card_calls();
        calls.push(completion(&snapshot(history, "goal")));
        multi_reply(calls)
    });
    let run = run_script(
        &files,
        &mut session,
        "解释并生成卡片",
        vec![create(), start_cards(), read_goal_with_text(), batch],
    )
    .await
    .unwrap();
    assert_waiting_drafts(&files, &session, &run);
    assert!(results(&session)
        .iter()
        .any(|r| r["error"]["code"] == "GOAL_INCOMPLETE"));
}

/// 独立生成成功场景只提供到生成批次的响应，多一次模型请求都会令脚本失败。
#[tokio::test]
async fn generated_drafts_immediately_wait_for_user() {
    let (_root, files, mut session) = generation_fixture();
    let run = run_script(
        &files,
        &mut session,
        "生成卡片并等待我采纳",
        vec![create(), start_cards(), fixed(multi_reply(card_calls()))],
    )
    .await
    .unwrap();
    assert_waiting_drafts(&files, &session, &run);
}

/// 仅恢复测试手动模拟已保存草稿；生产采纳路径仍由用户确认服务负责写入标记。
async fn saved_draft(files: &AgentFiles, session: &mut AgentSession, resolved: bool) {
    run_script(files, session, "解释并生成卡片", vec![create(), wait()])
        .await
        .unwrap();
    let goal = session.goal.as_ref().unwrap();
    session.messages.push(tools::block(
        "drafts",
        "已保存草稿",
        json!({
            "goalId":goal.id,"goalRevision":goal.revision,"adoptionResolved":resolved,
            "cards":[{"id":"draft-card"}],"path":"a.md"
        }),
    ));
    files.save_session(session).unwrap();
    *session = files.session(&session.id).unwrap();
}

/// 恢复依据当前用户授权，但只武装续轮，不把未采纳草稿自动标记为已完成。
fn resume(pending: bool) -> Response {
    Box::new(move |history, definitions| {
        assert!(!definitions
            .iter()
            .any(|tool| tool.name == "finish_generation"));
        let state = snapshot(history, "goal");
        assert_eq!(state["pendingDrafts"], pending);
        reply(
            "update_goal",
            json!({"goalId":state["goal"]["id"],
            "revision":state["goal"]["revision"],"action":"resume"}),
        )
    })
}

/// 重新加载时局部生成状态已清空，持久化待采纳标记仍必须拦住完成。
#[tokio::test]
async fn unresolved_reloaded_drafts_block_complete_after_resume() {
    let (_root, files, mut session) = generation_fixture();
    saved_draft(&files, &mut session, false).await;
    let attempt: Response = Box::new(|history, definitions| {
        assert!(!definitions
            .iter()
            .any(|tool| tool.name == "finish_generation"));
        let state = snapshot(history, "goal");
        assert_eq!(state["pendingDrafts"], true);
        assert_eq!(state["armed"], true);
        multi_reply(vec![completion(&state)])
    });
    let run = run_script(
        &files,
        &mut session,
        "明确继续完成",
        vec![
            read_goal_with_text(),
            resume(true),
            read_goal_with_text(),
            attempt,
            wait(),
        ],
    )
    .await
    .unwrap();
    assert_eq!(run.waiting_reason.as_deref(), Some("waitingUser"));
    assert_eq!(session.goal.as_ref().unwrap().phase, GoalPhase::Active);
    assert!(results(&session)
        .iter()
        .any(|r| r["error"]["code"] == "GOAL_INCOMPLETE"));
    let drafts = session
        .messages
        .iter()
        .find(|m| m.kind == "drafts")
        .unwrap();
    assert_eq!(drafts.data["adoptionResolved"], false);
}

/// 用户采纳确认或显式重定义目标后，历史草稿不永久阻塞新验收；旧草稿本身仍保留。
#[tokio::test]
async fn resolved_or_redefined_drafts_do_not_block_current_goal() {
    for resolved in [true, false] {
        let (_root, files, mut session) = generation_fixture();
        saved_draft(&files, &mut session, resolved).await;
        let mut script = vec![read_goal_with_text(), resume(!resolved)];
        if !resolved {
            script.push(Box::new(|history, _| {
                let state = snapshot(history, "goal");
                reply("update_goal", json!({"goalId":state["goal"]["id"],
                    "revision":state["goal"]["revision"],"action":"edit", "objective":"只需要解释，无需采纳卡片",
                    "acceptanceCriteria":["给出解释"]}))
            }));
        }
        script.push(read_goal_with_text());
        script.push(Box::new(|history, _| {
            let state = snapshot(history, "goal");
            assert_eq!(state["pendingDrafts"], false);
            multi_reply(vec![completion(&state)])
        }));
        run_script(
            &files,
            &mut session,
            "已采纳；或明确调整目标为仅解释",
            script,
        )
        .await
        .unwrap();
        let stored = files.session(&session.id).unwrap();
        assert_eq!(stored.goal.as_ref().unwrap().phase, GoalPhase::Complete);
        let drafts = stored.messages.iter().find(|m| m.kind == "drafts").unwrap();
        assert_eq!(drafts.data["adoptionResolved"], resolved);
        if !resolved {
            assert!(
                stored
                    .messages
                    .iter()
                    .filter(|m| m.data["goalReset"] == true)
                    .count()
                    >= 2
            );
        }
    }
}
