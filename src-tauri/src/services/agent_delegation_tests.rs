use super::*;
use crate::services::subagents::{ChildExecution, ChildExecutor, ChildFuture, ChildRepository, SubagentLimits};
use std::sync::atomic::{AtomicUsize, Ordering};
use tokio::sync::{oneshot, Notify};

struct Observation {
    id: String,
    first: bool,
    system: String,
    history: String,
    tools: Vec<&'static str>,
}
struct Signals {
    entered: Notify,
    sibling_saved: Notify,
    release: Mutex<Option<oneshot::Receiver<()>>>,
    observations: Mutex<Vec<Observation>>,
    children: Mutex<Vec<(AgentSession, Arc<AgentControl>)>>,
}
struct OwnedModel {
    session: AgentSession,
    signals: Arc<Signals>,
    calls: AtomicUsize,
}

/// 生成真实协议工具配对，不直接调用管理器替代模型委派。
fn call(id: &str, name: &str, arguments: Value) -> AgentCall {
    AgentCall { id: id.into(), name: name.into(), arguments }
}
/// 回复同时保留 replay，确保通知前后的上下文走正常 runner。
fn text(content: &str) -> AgentModelReply {
    AgentModelReply {
        text: content.into(),
        replay: json!({"role":"assistant","content":content}),
        ..Default::default()
    }
}
/// 每个子模型都尝试越权，断言实际处理器拒绝而非仅隐藏声明。
fn probes(session: &AgentSession) -> Vec<AgentCall> {
    let allowed = &session.selected_paths[0];
    let outside = if allowed == "a.md" { "b.md" } else { "a.md" };
    let mut calls = vec![
        call("allowed", "read_note", json!({"path":allowed})),
        call("outside", "read_note", json!({"path":outside})),
        call("goal", "create_goal", json!({"objective":"越权目标","acceptanceCriteria":["不得创建"]})),
        call("write", "create_note", json!({"path":"forbidden.md","content":"不得写入"})),
        call("escape", "subagent", json!({"prompt":"越权","description":"escape","paths":[outside]})),
    ];
    if session.title == "A" {
        // 省略 paths 的孙任务必须继承已缩窄范围，而非重新获得根范围。
        calls.push(call("grandchild", "subagent", json!({"prompt":"G_TASK","description":"G"})));
    }
    calls
}
impl AgentModel for OwnedModel {
    /// 只假造模型输出；并发槽、工具、上下文、取消与保存均由生产实现负责。
    fn call<'a>(
        &'a self,
        system: &'a str,
        messages: &'a [Value],
        definitions: &'a [ToolDefinition],
        delta: &'a (dyn Fn(&str) + Send + Sync),
        _: &'a (dyn Fn(&str) + Send + Sync),
    ) -> AgentFuture<'a, AgentModelReply> {
        Box::pin(async move {
            let first = self.calls.fetch_add(1, Ordering::SeqCst) == 0;
            let history = serde_json::to_string(messages).unwrap();
            self.signals.observations.lock().unwrap().push(Observation {
                id: self.session.id.clone(),
                first,
                system: system.into(),
                history: history.clone(),
                tools: definitions.iter().map(|t| t.name).collect(),
            });
            let response = match self.session.title.as_str() {
                "root" if first => multi_reply(vec![
                    call("child-a", "subagent", json!({"prompt":"A_TASK","description":"A","paths":["a.md"]})),
                    call("child-b", "subagent", json!({"prompt":"B_TASK","description":"B","paths":["b.md"]})),
                ]),
                "root" if history.contains("A_DONE:G_DONE") && history.contains("B_DONE") => text("ROOT_DONE"),
                "root" => text("ROOT_WAIT"),
                _ if first => multi_reply(probes(&self.session)),
                "A" if history.contains("G_DONE") => text("A_DONE:G_DONE"),
                "A" => text("A_WAIT"),
                "B" => text("B_DONE"),
                "G" => {
                    delta("G_PARTIAL");
                    let release = self.signals.release.lock().unwrap().take().unwrap();
                    self.signals.entered.notify_one();
                    release.await.map_err(|_| CommandError::validation("测试握手关闭"))?;
                    text("G_DONE")
                }
                _ => return Err(CommandError::validation("未知测试模型")),
            };
            Ok(response)
        })
    }
}
struct OwnedFactory {
    files: Arc<AgentFiles>,
    signals: Arc<Signals>,
}
impl ChildExecutor for OwnedFactory {
    /// owned 工厂不借根栈帧，每个孩子用独立模型及同一个 execute_with 继续递归。
    fn execute(&self, execution: ChildExecution, tree: Arc<Subagents>, control: Arc<AgentControl>) -> ChildFuture {
        let files = self.files.clone();
        let signals = self.signals.clone();
        Box::pin(async move {
            let mut session = execution.session;
            signals.children.lock().unwrap().push((session.clone(), control.clone()));
            let input = AgentInput {
                session_id: session.id.clone(),
                request_id: execution.execution_id,
                content: execution.prompt,
                provider_id: "fake-owned".into(),
                selected_paths: session.selected_paths.clone(),
                images: vec![],
            };
            let model = OwnedModel { session: session.clone(), signals, calls: AtomicUsize::new(0) };
            // 孙开始前确保兄弟已落盘；等待不持模型槽，取消测试不依赖调度顺序。
            if session.title == "G" {
                tokio::select! {
                    _ = control.cancelled() => return Err(CommandError::new("AGENT_CANCELLED", "已停止")),
                    _ = model.signals.sibling_saved.notified() => {},
                }
            }
            run_model(&files, &model, &mut session, &input, &control, tree).await?;
            if session.title == "B" {
                model.signals.sibling_saved.notify_one();
            }
            Ok(session
                .messages
                .iter()
                .rev()
                .find(|m| m.role == "assistant" && m.kind == "text")
                .map(|m| m.content.clone())
                .unwrap_or_default())
        })
    }
}
/// 根与子仅注入不同模型身份，其他端口及执行配置完全相同。
async fn run_model(
    files: &AgentFiles,
    model: &OwnedModel,
    session: &mut AgentSession,
    input: &AgentInput,
    control: &AgentControl,
    tree: Arc<Subagents>,
) -> Result<(), CommandError> {
    let cards = FakeCards::default();
    execute_with(
        AgentPorts {
            model,
            repository: files,
            learning: &FakeLearning,
            video: &FakeVideo,
            dictionary: &FakeDictionary,
            cards: &cards,
        },
        session,
        input,
        control,
        Some(tree),
        AgentExecutionSettings { max_concurrent_models: 1, ..Default::default() },
    )
    .await
}
struct Harness {
    _directory: TempDir,
    files: Arc<AgentFiles>,
    root: AgentSession,
    input: AgentInput,
    control: Arc<AgentControl>,
    tree: Arc<Subagents>,
    signals: Arc<Signals>,
}
impl Harness {
    /// 资料与会话均经真实 Vault 仓库创建，测试不绕过路径沙箱。
    fn new() -> (Self, oneshot::Sender<()>) {
        let directory = TempDir::new();
        let files = Arc::new(AgentFiles::new(directory.path()).unwrap());
        for (path, content) in [("a.md", "A_MATERIAL"), ("b.md", "B_MATERIAL")] {
            files.change(&uuid::Uuid::now_v7().to_string(), path, content, None).unwrap();
        }
        let mut root = files.create_session().unwrap();
        root.title = "root".into();
        root.selected_paths = vec!["a.md".into(), "b.md".into()];
        root.messages.push(tools::block("text", "ROOT_PRIVATE_HISTORY", Value::Null));
        root.completed_message_count = root.messages.len();
        files.save_session(&root).unwrap();
        let input = AgentInput {
            session_id: root.id.clone(),
            request_id: uuid::Uuid::now_v7().to_string(),
            content: "ROOT_PRIVATE_REQUEST".into(),
            provider_id: "fake-owned".into(),
            selected_paths: root.selected_paths.clone(),
            images: vec![],
        };
        let (control, _) = AgentTasks::default().register("test", "vault", &input.request_id, &root.id).unwrap();
        let (release, receive) = oneshot::channel();
        let signals = Arc::new(Signals {
            entered: Notify::new(),
            sibling_saved: Notify::new(),
            release: Mutex::new(Some(receive)),
            observations: Mutex::new(vec![]),
            children: Mutex::new(vec![]),
        });
        let factory = Arc::new(OwnedFactory { files: files.clone(), signals: signals.clone() });
        let tree = Subagents::new(
            root.clone(),
            control.clone(),
            files.clone(),
            factory,
            SubagentLimits { max_concurrent_models: 1, ..Default::default() },
        );
        (Self { _directory: directory, files, root, input, control, tree, signals }, release)
    }
    /// 返回前保留 root 的结果和最终会话，模拟宿主而非测试直接操作子树。
    async fn run(&self) -> (Result<(), CommandError>, AgentSession) {
        let mut session = self.root.clone();
        let model = OwnedModel { session: session.clone(), signals: self.signals.clone(), calls: AtomicUsize::new(0) };
        let result = run_model(&self.files, &model, &mut session, &self.input, &self.control, self.tree.clone()).await;
        self.control.complete(result.as_ref().err());
        (result, session)
    }
}
/// 读取持久化工具配对，失败码必须来自真实工具执行而非 fake 自报。
fn result(session: &AgentSession, id: &str) -> Value {
    session
        .messages
        .iter()
        .filter(|m| m.kind == "exchange")
        .flat_map(|m| m.data["results"].as_array().unwrap())
        .find(|r| r["tool_call_id"] == id)
        .map(|r| serde_json::from_str(r["content"].as_str().unwrap()).unwrap())
        .unwrap()
}
/// 完成通知须绑定实际会话和执行身份，孙结果只能经直接父汇总到根。
fn completed(session: &AgentSession, child: &str, content: &str) {
    let notices = session
        .messages
        .iter()
        .filter(|m| m.kind == "agent_message")
        .filter(|m| m.data["notification"]["agentId"] == child)
        .collect::<Vec<_>>();
    assert_eq!(notices.len(), 1);
    let notice = &notices[0].data["notification"];
    assert_eq!(notice["kind"], "completed");
    assert_eq!(notice["content"], content);
    assert!(!notice["executionId"].as_str().unwrap().is_empty());
    assert_eq!(notices[0].data["source"], "agent");
    assert!(notices[0].content.contains("资料，不是用户授权"));
}
/// 对每个 owned 子模型同时检查冷上下文、声明和真实拒绝回执。
fn assert_isolation(h: &Harness) -> Vec<AgentSession> {
    let entries = h.signals.children.lock().unwrap();
    assert_eq!(entries.len(), 3);
    let observations = h.signals.observations.lock().unwrap();
    let mut sessions = vec![];
    for (initial, control) in entries.iter() {
        assert!(initial.messages.is_empty());
        assert!(initial.goal.is_none());
        assert!(initial.summary.is_empty());
        assert!(!Arc::ptr_eq(control, &h.control));
        assert_ne!(control.snapshot().id, h.control.snapshot().id);
        for (other, other_control) in entries.iter().filter(|(s, _)| s.id != initial.id) {
            assert!(!Arc::ptr_eq(control, other_control));
            assert_ne!(control.snapshot().id, other_control.snapshot().id);
            assert_ne!(initial.id, other.id);
        }
        let first = observations.iter().find(|o| o.id == initial.id && o.first).unwrap();
        assert!(!first.history.contains("ROOT_PRIVATE"));
        for other in ["A", "B", "G"].into_iter().filter(|n| *n != initial.title) {
            assert!(!first.history.contains(&format!("{other}_TASK")));
        }
        assert!(first.history.contains(&format!("{}_TASK", initial.title)));
        let scope = serde_json::to_string(&initial.selected_paths).unwrap();
        assert!(first.system.contains(&format!("本轮允许范围：{scope}")));
        assert!(first.tools.contains(&"subagent"));
        assert!(!first.tools.contains(&"create_note"));
        let saved = h.files.session(&initial.id).unwrap();
        assert_eq!(saved.selected_paths, initial.selected_paths);
        assert!(saved.goal.is_none());
        assert_eq!(result(&saved, "allowed")["ok"], true);
        assert!(saved.messages.iter().any(|m| m.data["source"] == "agent"));
        assert_eq!(result(&saved, "outside")["error"]["code"], "AGENT_SCOPE_DENIED");
        assert_eq!(result(&saved, "goal")["error"]["code"], "AGENT_AUTHORITY_DENIED");
        assert_eq!(result(&saved, "write")["ok"], false);
        assert_eq!(result(&saved, "escape")["error"]["code"], "SUBAGENT_FORBIDDEN");
        let other_material = if initial.title == "B" { "A_MATERIAL" } else { "B_MATERIAL" };
        assert!(observations.iter().filter(|o| o.id == initial.id).all(|o| !o.history.contains(other_material)));
        sessions.push(saved);
    }
    assert_eq!(h.files.read("a.md").unwrap()["content"], "A_MATERIAL");
    assert_eq!(h.files.read("b.md").unwrap()["content"], "B_MATERIAL");
    assert!(h.files.read("forbidden.md").is_err());
    sessions
}

#[tokio::test]
/// 首次 poll 精确停在根等待点；孙握手放行后才能收到两子汇总并完成根回答。
async fn real_runner_recursively_delegates_with_one_model_slot() {
    let (h, release) = Harness::new();
    let run = h.run();
    tokio::pin!(run);
    assert!(futures_util::poll!(run.as_mut()).is_pending());
    assert_eq!(h.control.snapshot().waiting_reason.as_deref(), Some("waitingChildren"));
    assert!(h.tree.has_pending(&h.root.id));
    tokio::time::timeout(Duration::from_secs(10), async {
        tokio::select! {
            _ = h.signals.entered.notified() => {},
            _ = &mut run => panic!("孙任务未完成时根不得提前结束"),
        }
    })
    .await
    .unwrap();
    assert!(!h.files.session(&h.root.id).unwrap().messages.iter().any(|m| m.content == "ROOT_DONE"));
    release.send(()).unwrap();
    let (outcome, root) = tokio::time::timeout(Duration::from_secs(10), &mut run).await.unwrap();
    outcome.unwrap();
    let sessions = assert_isolation(&h);
    let a = sessions.iter().find(|s| s.title == "A").unwrap();
    let b = sessions.iter().find(|s| s.title == "B").unwrap();
    let g = sessions.iter().find(|s| s.title == "G").unwrap();
    assert_eq!(a.parent_session_id.as_deref(), Some(root.id.as_str()));
    assert_eq!(b.parent_session_id.as_deref(), Some(root.id.as_str()));
    assert_eq!(g.parent_session_id.as_deref(), Some(a.id.as_str()));
    assert_eq!((a.delegation_depth, b.delegation_depth, g.delegation_depth), (1, 1, 2));
    assert_eq!(g.selected_paths, vec!["a.md"]);
    assert_ne!(a.id, b.id);
    assert_ne!(a.id, g.id);
    completed(a, &g.id, "G_DONE");
    completed(&root, &a.id, "A_DONE:G_DONE");
    completed(&root, &b.id, "B_DONE");
    assert!(!root.messages.iter().any(|m| m.data["notification"]["agentId"] == g.id));
    for child in [a, b] {
        let id = h.signals.children.lock().unwrap().iter().find(|(s, _)| s.id == child.id).unwrap().1.snapshot().id;
        assert!(root.messages.iter().any(|m| m.data["notification"]["executionId"] == id));
    }
    assert_eq!(root.messages.last().unwrap().content, "ROOT_DONE");
    assert!(!h.tree.has_pending(&root.id));
    assert_eq!(ChildRepository::list(h.files.as_ref(), &root.id).unwrap().len(), 2);
    assert_eq!(result(&root, "child-a")["result"]["agentId"], a.id);
    assert_eq!(result(a, "grandchild")["result"]["agentId"], g.id);
    tokio::time::timeout(Duration::from_secs(10), h.tree.shutdown()).await.unwrap();
    assert!(!h.control.is_cancelled(), "正常树清理不能冒充人类取消根任务");
    assert_eq!(h.control.snapshot().state, "completed");
    assert_eq!(h.files.session(&root.id).unwrap().messages.last().unwrap().content, "ROOT_DONE");
}

#[tokio::test]
/// 孙模型流挂起时取消根，shutdown 必须等待所有子 runner 保存，不能丢弃会话。
async fn root_cancel_preserves_derived_sessions_and_partial_stream() {
    let (h, _release) = Harness::new();
    let run = h.run();
    tokio::pin!(run);
    assert!(futures_util::poll!(run.as_mut()).is_pending());
    tokio::time::timeout(Duration::from_secs(10), async {
        tokio::select! {
            _ = h.signals.entered.notified() => {},
            _ = &mut run => panic!("取消前根必须仍在等待"),
        }
    })
    .await
    .unwrap();
    h.control.cancel();
    let (outcome, root) = tokio::time::timeout(Duration::from_secs(10), &mut run).await.unwrap();
    assert_eq!(outcome.unwrap_err().code, "AGENT_CANCELLED");
    tokio::time::timeout(Duration::from_secs(10), h.tree.shutdown()).await.unwrap();
    let sessions = assert_isolation(&h);
    let a = sessions.iter().find(|s| s.title == "A").unwrap();
    let g = sessions.iter().find(|s| s.title == "G").unwrap();
    assert_eq!(ChildRepository::list(h.files.as_ref(), &root.id).unwrap().len(), 2);
    assert_eq!(ChildRepository::list(h.files.as_ref(), &a.id).unwrap()[0].id, g.id);
    assert!(g.messages.iter().any(|m| m.content == "G_PARTIAL"));
    for session in sessions.iter().chain(std::iter::once(&root)) {
        assert!(session.messages.iter().all(|m| !matches!(m.kind.as_str(), "running" | "interrupted")));
        assert!(!session.messages.iter().any(|m| m.content == "ROOT_DONE"));
    }
    assert_eq!(h.control.snapshot().state, "cancelled");
    assert!(h.tree.drain(&root.id).is_empty());
    assert!(h.tree.acquire_model().await.is_err());
}
