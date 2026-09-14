use super::*;

#[tokio::test]
/// 新目标不能引用旧目标文本，也不能用 get_goal 状态收据自证业务完成。
async fn rejects_previous_goal_output_and_state_tool_receipts() {
    let temp = TempDir::new();
    let files = AgentFiles::new(temp.path()).unwrap();
    let mut session = files.create_session().unwrap();
    let old = tools::block("text", "上一个任务的答案", Value::Null);
    let old_id = old.id.clone();
    session.messages.push(old);
    files.save_session(&session).unwrap();
    let responses: Vec<Response> = vec![
        create(), fixed(reply("get_goal", json!({}))),
        Box::new(move |history, _| {
            let data = snapshot(history, "goal");
            assert!(!data["receipts"].to_string().contains(&old_id));
            reply("update_goal", json!({"goalId":data["goal"]["id"],"revision":data["goal"]["revision"],"action":"complete",
                "evidence":[{"criterionIndex":0,"goalRevision":data["goal"]["revision"],"sourceVersion":data["sourceVersion"],"receiptRef":format!("message:{old_id}")}] }))
        }),
        Box::new(|history, _| {
            let last: Value = serde_json::from_str(history.last().unwrap()["content"].as_str().unwrap()).unwrap();
            assert_eq!(last["error"]["code"], "GOAL_INCOMPLETE");
            reply("get_goal", json!({}))
        }),
        Box::new(|history, _| {
            let data = snapshot(history, "goal");
            let id = history.iter().rev().find(|m| m["role"] == "tool").unwrap()["tool_call_id"].as_str().unwrap();
            reply("update_goal", json!({"goalId":data["goal"]["id"],"revision":data["goal"]["revision"],"action":"complete",
                "evidence":[{"criterionIndex":0,"goalRevision":data["goal"]["revision"],"sourceVersion":data["sourceVersion"],"receiptRef":format!("tool:{id}")}] }))
        }),
        Box::new(|history, _| {
            let last: Value = serde_json::from_str(history.last().unwrap()["content"].as_str().unwrap()).unwrap();
            assert_eq!(last["error"]["code"], "GOAL_INCOMPLETE");
            reply("wait_for_user", json!({"question":"请确认新的验收范围"}))
        }),
    ];
    let run = run_script(&files, &mut session, "开始新的独立任务", responses).await.unwrap();
    // fixture 只运行执行器，根控制器终态由组合层在子树收尾后发布。
    assert_eq!(run.waiting_reason.as_deref(), Some("waitingUser"));
    assert_eq!(session.goal.as_ref().unwrap().phase, GoalPhase::Active);
}
