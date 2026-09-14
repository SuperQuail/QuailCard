// 不限执行预算的回归测试；仍以显式完成、失败或取消结束。
use super::*;

struct SlowModel;
impl AgentModel for SlowModel {
    /// 虚拟时钟跨过原先的十五分钟上限，验证模型等待不再由 Agent 截断。
    fn call<'a>(
        &'a self,
        _: &'a str,
        _: &'a [Value],
        _: &'a [ToolDefinition],
        _: &'a (dyn Fn(&str) + Send + Sync),
        _: &'a (dyn Fn(&str) + Send + Sync),
    ) -> AgentFuture<'a, AgentModelReply> {
        Box::pin(async {
            tokio::time::sleep(Duration::from_secs(901)).await;
            Ok(AgentModelReply {
                text: "长任务完成".into(),
                ..Default::default()
            })
        })
    }
}

/// 旧设置只作为兼容输入，调用数与时间数值均不能再限制执行。
async fn run_with_legacy_settings(
    files: &AgentFiles,
    session: &mut AgentSession,
    model: &dyn AgentModel,
    control: &AgentControl,
) -> Result<(), CommandError> {
    let input: AgentInput = serde_json::from_value(json!({
        "sessionId":session.id,"requestId":"unbounded","content":"完成任务",
        "providerId":"test","selectedPaths":[]
    }))
    .unwrap();
    execute_with(
        AgentPorts {
            model,
            repository: files,
            learning: &FakeLearning,
            video: &FakeVideo,
            dictionary: &FakeDictionary,
            cards: &FakeCards::default(),
        },
        session,
        &input,
        control,
        None,
        AgentExecutionSettings {
            max_model_calls: 1,
            timeout_seconds: 10,
            max_goal_rounds: 1,
            ..Default::default()
        },
    )
    .await
}

/// 两次调用超过旧配置一次上限，仍可正常回答并持久化。
#[tokio::test]
async fn legacy_call_budget_does_not_stop_execution() {
    let root = TempDir::new();
    let files = AgentFiles::new(root.path()).unwrap();
    let mut session = files.create_session().unwrap();
    let tasks = AgentTasks::default();
    let (control, _) = tasks
        .register("main", "root", "unbounded", &session.id)
        .unwrap();
    let model = FakeModel {
        replies: Mutex::new(VecDeque::from([
            reply("get_plan", json!({})),
            AgentModelReply {
                text: "完成".into(),
                ..Default::default()
            },
        ])),
    };
    run_with_legacy_settings(&files, &mut session, &model, &control)
        .await
        .unwrap();
    assert!(model.replies.lock().unwrap().is_empty());
    assert!(files
        .session(&session.id)
        .unwrap()
        .messages
        .iter()
        .any(|m| m.content == "完成"));
}

/// 长模型请求越过旧时限仍成功，不依赖真实时间等待。
#[tokio::test(start_paused = true)]
async fn model_response_can_exceed_legacy_timeout() {
    let root = TempDir::new();
    let files = AgentFiles::new(root.path()).unwrap();
    let mut session = files.create_session().unwrap();
    let tasks = AgentTasks::default();
    let (control, _) = tasks
        .register("main", "root", "unbounded", &session.id)
        .unwrap();
    run_with_legacy_settings(&files, &mut session, &SlowModel, &control)
        .await
        .unwrap();
    assert!(session.messages.iter().any(|m| m.content == "长任务完成"));
}

/// 去掉时限后，模型仍未响应时用户取消必须立即生效。
#[tokio::test(start_paused = true)]
async fn cancellation_still_interrupts_model_wait() {
    let root = TempDir::new();
    let files = AgentFiles::new(root.path()).unwrap();
    let mut session = files.create_session().unwrap();
    let tasks = AgentTasks::default();
    let (control, _) = tasks
        .register("main", "root", "unbounded", &session.id)
        .unwrap();
    let cancel = async {
        tokio::time::sleep(Duration::from_secs(20)).await;
        control.cancel();
    };
    let (result, ()) = tokio::join!(
        run_with_legacy_settings(&files, &mut session, &SlowModel, &control),
        cancel
    );
    assert_eq!(result.unwrap_err().code, "AGENT_CANCELLED");
    assert!(!session.messages.iter().any(|m| m.content == "长任务完成"));
}
