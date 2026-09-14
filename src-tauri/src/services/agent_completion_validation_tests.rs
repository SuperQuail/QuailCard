use super::*;

/// 同一轮反复申请阻塞不是三轮尝试，错误必须说明当前轮数与真实门槛。
#[tokio::test]
async fn completion_blocked_retries_keep_zero_rounds_and_explain_threshold() {
    let root = TempDir::new();
    let files = AgentFiles::new(root.path()).unwrap();
    let mut session = files.create_session().unwrap();
    let mut script = vec![create()];
    for _ in 0..4 {
        script.push(Box::new(|history, _| {
            let state = snapshot(history, "goal");
            completion_assert_active(&state);
            reply(
                "update_goal",
                json!({"goalId":state["goal"]["id"],"revision":state["goal"]["revision"],
                    "action":"blocked","blocker":{"reason":"需要用户确定范围","attempts":[]}}),
            )
        }));
        script.push(completion_rejected(
            "GOAL_BLOCKER_TOO_EARLY",
            "roundsStarted=0",
        ));
    }
    script.push(wait());
    run_script(&files, &mut session, "完成解释，范围不明时请询问", script)
        .await
        .unwrap();
    let receipts = results(&session);
    let errors: Vec<_> = receipts.iter().filter(|r| r["ok"] == false).collect();
    assert_eq!(errors.len(), 4);
    for error in errors {
        let message = error["error"]["message"].as_str().unwrap();
        assert!(message.contains("roundsStarted=0"));
        assert!(message.contains('3'), "缺少三轮门槛: {message}");
    }
    let stored = files.session(&session.id).unwrap();
    assert_eq!(
        serde_json::to_value(stored.goal.unwrap()).unwrap(),
        receipts[0]["result"]["goal"]
    );
    assert!(!stored.messages.iter().any(|m| m.kind == "goal_round"));
}

/// 每次省略不同条件，其他合法证据不能代替缺失条件，补齐后原目标可继续完成。
#[tokio::test]
async fn completion_each_missing_criterion_rejected_then_full_coverage_recovers() {
    let root = TempDir::new();
    let files = AgentFiles::new(root.path()).unwrap();
    let mut session = files.create_session().unwrap();
    completion_note(&files, &session);
    let mut script = vec![
        completion_create_four(),
        fixed(reply("read_note", json!({"path":"completion.md"}))),
        fixed(reply("get_goal", json!({}))),
    ];
    for missing in 0..4 {
        script.push(Box::new(move |history, _| {
            let state = snapshot(history, "goal");
            completion_assert_active(&state);
            let mut evidence = completion_evidence(&state);
            evidence.as_array_mut().unwrap().remove(missing);
            reply(
                "update_goal",
                json!({"goalId":state["goal"]["id"],"revision":state["goal"]["revision"],
                    "action":"complete","evidence":evidence}),
            )
        }));
        script.push(completion_rejected(
            "GOAL_INCOMPLETE",
            "acceptanceCriteria[",
        ));
    }
    script.push(completion_finish(None));
    run_script(&files, &mut session, "逐项完成四个验收条件", script)
        .await
        .unwrap();
    let goal = session.goal.as_ref().unwrap();
    assert_eq!(goal.phase, GoalPhase::Complete);
    assert_eq!(goal.evidence.len(), 4);
    let receipts = results(&session);
    for (index, error) in receipts.iter().filter(|r| r["ok"] == false).enumerate() {
        assert!(error["error"]["message"]
            .as_str()
            .unwrap()
            .contains(&format!("acceptanceCriteria[{index}]")));
    }
    assert_eq!(receipts.iter().filter(|r| r["ok"] == false).count(), 4);
}

/// 合法收据仍须绑定当前修订、请求与有效条件索引，逐项拒绝后可恢复。
#[tokio::test]
async fn completion_evidence_metadata_must_match_current_goal_and_request() {
    let root = TempDir::new();
    let files = AgentFiles::new(root.path()).unwrap();
    let mut session = files.create_session().unwrap();
    completion_note(&files, &session);
    let mut script = vec![
        completion_create_four(),
        fixed(reply("read_note", json!({"path":"completion.md"}))),
        fixed(reply("get_goal", json!({}))),
    ];
    for (field, invalid) in [
        ("goalRevision", json!(99)),
        ("sourceVersion", json!("another-request")),
        ("criterionIndex", json!(4)),
    ] {
        script.push(Box::new(move |history, _| {
            let state = snapshot(history, "goal");
            completion_assert_active(&state);
            let mut evidence = completion_evidence(&state);
            evidence[1][field] = invalid;
            reply(
                "update_goal",
                json!({"goalId":state["goal"]["id"],"revision":state["goal"]["revision"],
                    "action":"complete","evidence":evidence}),
            )
        }));
        script.push(completion_rejected("GOAL_INCOMPLETE", "evidence[1]"));
    }
    script.push(completion_finish(None));
    run_script(&files, &mut session, "用当前证据完成四项解释", script)
        .await
        .unwrap();
    assert_eq!(session.goal.as_ref().unwrap().phase, GoalPhase::Complete);
    assert_eq!(session.goal.as_ref().unwrap().revision, 2);
    assert_eq!(
        results(&session)
            .iter()
            .filter(|r| r["ok"] == false)
            .count(),
        3
    );
}

/// 重载后的旧读取收据仍复验磁盘 hash，材料变化后必须重新读取才能完成。
#[tokio::test]
async fn completion_stale_note_hash_rejected_then_fresh_read_recovers() {
    let root = TempDir::new();
    let files = AgentFiles::new(root.path()).unwrap();
    let mut session = files.create_session().unwrap();
    completion_note(&files, &session);
    run_script(
        &files,
        &mut session,
        "核实解释，等我修改补充后再继续",
        vec![
            create(),
            fixed(reply("read_note", json!({"path":"completion.md"}))),
            wait(),
        ],
    )
    .await
    .unwrap();
    let old = files.read("completion.md").unwrap();
    // change 的身份是幂等操作 ID，不是会话 ID；新编辑必须使用新身份。
    files
        .change(
            &uuid::Uuid::now_v7().to_string(),
            "completion.md",
            "用户已修正解释",
            old["hash"].as_str(),
        )
        .unwrap();
    assert_ne!(files.read("completion.md").unwrap()["hash"], old["hash"]);
    let mut restored = files.session(&session.id).unwrap();
    let before = restored.goal.clone().unwrap();
    run_script(
        &files,
        &mut restored,
        "请继续核实最新材料并完成原目标",
        vec![
            fixed(reply("get_goal", json!({}))),
            completion_finish(None),
            completion_rejected("GOAL_INCOMPLETE", "evidence[0]"),
            fixed(reply("read_note", json!({"path":"completion.md"}))),
            fixed(reply("get_goal", json!({}))),
            completion_finish(None),
        ],
    )
    .await
    .unwrap();
    let goal = files.session(&session.id).unwrap().goal.unwrap();
    assert_eq!(goal.id, before.id);
    assert_eq!(goal.revision, before.revision + 1);
    assert_eq!(goal.phase, GoalPhase::Complete);
    assert_eq!(
        results(&restored)
            .iter()
            .filter(|r| r["ok"] == false)
            .count(),
        1
    );
}
