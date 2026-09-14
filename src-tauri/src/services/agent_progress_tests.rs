use super::*;

/// 构造进入拆卡模式的调用，验证生成生命周期不受轮数约束。
fn enter() -> AgentModelReply {
    reply("generate_cards", json!({"path":"a.md","kind":"qa"}))
}

/// 同批规划并生成有效草稿，验证后续重试不会丢弃它。
fn accepted() -> AgentModelReply {
    multi_reply(vec![
        AgentCall {
            id: "plan".into(),
            name: "plan_cards".into(),
            arguments: json!({"items":[{"source":"学习材料","keyword":"考点"}]}),
        },
        AgentCall {
            id: "card".into(),
            name: "emit_card".into(),
            arguments: json!({"schema_version":1,"type_id":"qa","source":"学习材料","fields":{"front":"问题","back":"答案","detail":""}}),
        },
    ])
}

/// 完成工具收据证明没有按旧阈值早停；有草稿时必须直接等待，不能再请求总结。
async fn completed_session(mut replies: Vec<AgentModelReply>) -> AgentSession {
    let expected_exchanges = replies
        .iter()
        .filter(|reply| !reply.calls.is_empty())
        .count()
        + 1;
    replies.push(reply("finish_generation", json!({"reason":"完成"})));
    replies.push(AgentModelReply {
        text: "正常完成后的回答".into(),
        ..Default::default()
    });
    let root = TempDir::new();
    let files = AgentFiles::new(root.path()).unwrap();
    let session = execute_fixture(&files, vec![], replies).await;
    assert_eq!(
        session
            .messages
            .iter()
            .filter(|m| m.kind == "exchange")
            .count(),
        expected_exchanges
    );
    let has_drafts = session.messages.iter().any(|m| m.kind == "drafts");
    assert_eq!(
        session
            .messages
            .iter()
            .any(|m| m.content == "正常完成后的回答"),
        !has_drafts
    );
    session
}

#[tokio::test]
/// 多轮更新其他计划不会触发生成自动停止。
async fn unrelated_rounds_do_not_stop_generation() {
    let mut replies = vec![enter(), accepted()];
    replies.extend((0..12).map(|index| {
        reply(
            "update_plan",
            json!({"planRevision":index,"steps":[{"id":"step","text":format!("步骤 {index}"),"status":"in_progress"}]}),
        )
    }));
    let session = completed_session(replies).await;
    let drafts = session
        .messages
        .iter()
        .find(|m| m.kind == "drafts")
        .unwrap();
    assert_eq!(drafts.data["cards"].as_array().unwrap().len(), 1);
}

#[tokio::test]
/// 多轮错误和重复开始都不强制结束，也不能覆盖已有草稿。
async fn retries_and_reentry_do_not_stop_or_discard_drafts() {
    let mut replies = vec![enter()];
    replies.extend((0..5).map(|_| reply("unknown", json!({}))));
    replies.push(accepted());
    replies.extend((0..12).map(|_| enter()));
    let session = completed_session(replies).await;
    assert_eq!(
        session
            .messages
            .iter()
            .filter(|m| m.kind == "drafts")
            .count(),
        1
    );
}

#[tokio::test]
/// 纯文字不冒充拆卡完成，超过旧阈值仍可继续调用完成工具。
async fn text_only_generation_can_continue_past_old_limit() {
    let mut replies = vec![enter()];
    replies.extend((0..12).map(|_| AgentModelReply {
        text: "继续思考".into(),
        replay: json!({"role":"assistant","content":"继续思考"}),
        ..Default::default()
    }));
    let session = completed_session(replies).await;
    assert!(!session.messages.iter().any(|m| m.kind == "drafts"));
}

#[tokio::test]
/// 普通 Agent 不因无进展次数停下，由模型明确结束回答。
async fn ordinary_agent_continues_until_explicit_finish() {
    let root = TempDir::new();
    let files = AgentFiles::new(root.path()).unwrap();
    let mut replies = (0..8)
        .map(|_| reply("unknown", json!({})))
        .collect::<Vec<_>>();
    replies.push(AgentModelReply {
        text: "普通回答完成".into(),
        ..Default::default()
    });
    let session = execute_fixture(&files, vec![], replies).await;
    assert!(session.messages.iter().any(|m| m.content == "普通回答完成"));
}
