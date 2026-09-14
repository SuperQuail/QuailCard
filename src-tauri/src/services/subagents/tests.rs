use super::*;
use crate::{agent_models::AgentMessage, services::agent_tasks::AgentTasks};
use tokio::sync::{mpsc, oneshot};

#[derive(Default)]
struct FakeRepo(Mutex<BTreeMap<String, AgentSession>>);
impl ChildRepository for FakeRepo {
    /// 假仓库也拒绝覆盖身份，保证测试不依赖真实文件系统。
    fn create(&self, child: &AgentSession) -> Result<(), CommandError> {
        let mut rows = self.0.lock().unwrap();
        if rows.contains_key(&child.id) {
            return Err(forbidden());
        }
        rows.insert(child.id.clone(), child.clone());
        Ok(())
    }
    /// 克隆最新历史以暴露旧快照覆盖问题。
    fn load(&self, id: &str) -> Result<AgentSession, CommandError> {
        self.0
            .lock()
            .unwrap()
            .get(id)
            .cloned()
            .ok_or_else(forbidden)
    }
    /// 枚举只信子关系，故意完全忽略父 children。
    fn list(&self, parent_id: &str) -> Result<Vec<AgentSession>, CommandError> {
        Ok(self
            .0
            .lock()
            .unwrap()
            .values()
            .filter(|s| s.parent_session_id.as_deref() == Some(parent_id))
            .cloned()
            .collect())
    }
}
struct Started {
    execution: ChildExecution,
    control: Arc<AgentControl>,
    reply: oneshot::Sender<String>,
}
struct FakeExecutor {
    repo: Arc<FakeRepo>,
    started: mpsc::UnboundedSender<Started>,
}
impl ChildExecutor for FakeExecutor {
    /// 握手精确控制执行完成，不用 sleep 或模型轮询制造时序。
    fn execute(
        &self,
        execution: ChildExecution,
        _: Arc<Subagents>,
        control: Arc<AgentControl>,
    ) -> ChildFuture {
        let repo = self.repo.clone();
        let started = self.started.clone();
        Box::pin(async move {
            let mut session = execution.session.clone();
            session.messages.push(AgentMessage {
                content: execution.prompt.clone(),
                ..Default::default()
            });
            let (reply, received) = oneshot::channel();
            started
                .send(Started {
                    execution,
                    control: control.clone(),
                    reply,
                })
                .map_err(|_| forbidden())?;
            let result = tokio::select! {
                _ = control.cancelled() => Err(stopped()),
                reply = received => reply.map_err(|_| forbidden()),
            };
            session.completed_message_count = session.messages.len();
            repo.0.lock().unwrap().insert(session.id.clone(), session);
            result
        })
    }
}

/// 每个根 execution 使用独立 control，冷恢复仅共享 repository。
fn setup(
    root: AgentSession,
    repo: Arc<FakeRepo>,
    limits: SubagentLimits,
) -> (Arc<Subagents>, mpsc::UnboundedReceiver<Started>) {
    let (control, _) = AgentTasks::default()
        .register("window", "vault", &id(), &root.id)
        .unwrap();
    let (started, receiver) = mpsc::unbounded_channel();
    let executor = Arc::new(FakeExecutor {
        repo: repo.clone(),
        started,
    });
    (
        Subagents::new(root, control, repo, executor, limits),
        receiver,
    )
}
/// 固定根身份与白名单，校验旧父快照不能扩大授权。
fn root() -> AgentSession {
    AgentSession {
        id: "root".into(),
        selected_paths: vec!["a.md".into(), "b.md".into()],
        ..Default::default()
    }
}
/// 事件式等待子终态，不重复调用模型或等待固定时间。
async fn idle(manager: &Subagents, child: &str) {
    loop {
        if !manager.lock().nodes.get(child).unwrap().running {
            return;
        }
        manager.drain("root");
        manager.wait("root").await;
    }
}

#[tokio::test]
/// Fork 不复制当前工具批次、计划、摘要或父身份，也不会保存旧父快照。
async fn fork_copies_only_completed_history() {
    let mut parent = root();
    parent.summary = "当前轮次摘要".into();
    parent.children = vec!["old-child".into()];
    parent.messages = ["text", "plan", "text", "running"]
        .iter()
        .enumerate()
        .map(|(i, kind)| AgentMessage {
            content: i.to_string(),
            kind: (*kind).into(),
            ..Default::default()
        })
        .collect();
    parent.completed_message_count = 3;
    let repo = Arc::new(FakeRepo::default());
    let (manager, mut receiver) = setup(parent.clone(), repo.clone(), SubagentLimits::default());
    assert!(manager
        .spawn("root", &parent, "任务", "fork", true, vec!["a.md".into()])
        .await
        .is_err());
    let child = manager
        .spawn("root", &parent, "任务", "fork", true, vec![])
        .await
        .unwrap();
    let run = receiver.recv().await.unwrap();
    assert_eq!(run.execution.session.messages.len(), 2);
    assert_eq!(run.execution.session.completed_message_count, 2);
    assert!(run.execution.session.children.is_empty());
    assert!(run.execution.session.summary.is_empty());
    assert_eq!(
        run.execution.session.parent_session_id.as_deref(),
        Some("root")
    );
    assert_eq!(run.execution.session.id, child);
    assert_ne!(run.execution.execution_id, child);
    assert!(!run.execution.message_id.is_empty());
    assert!(repo.load("root").is_err());
    run.reply.send("完成".into()).unwrap();
    idle(&manager, &child).await;
    manager.shutdown().await;
}

#[tokio::test]
/// 递归配额在入口生效；祖先能中断深层节点，但反向中断和跨层消息被拒绝。
async fn recursive_authority_depth_and_interrupt() {
    let parent = root();
    let (manager, mut receiver) = setup(
        parent.clone(),
        Arc::new(FakeRepo::default()),
        SubagentLimits::default(),
    );
    let a = manager
        .spawn("root", &parent, "a", "a", false, vec![])
        .await
        .unwrap();
    let a_run = receiver.recv().await.unwrap();
    let b = manager
        .spawn(&a, &a_run.execution.session, "b", "b", false, vec![])
        .await
        .unwrap();
    let b_run = receiver.recv().await.unwrap();
    let c = manager
        .spawn(&b, &b_run.execution.session, "c", "c", false, vec![])
        .await
        .unwrap();
    let c_run = receiver.recv().await.unwrap();
    assert!(manager
        .spawn(&c, &c_run.execution.session, "d", "d", false, vec![])
        .await
        .is_err());
    assert!(manager.send("root", &b, "跨层").await.is_err());
    assert!(manager.send(&b, "root", "跨层").await.is_err());
    assert!(manager.interrupt(&b, &a).is_err());
    manager.interrupt("root", &b).unwrap();
    assert!(b_run.control.is_cancelled());
    assert!(!a_run.control.is_cancelled());
    assert!(!c_run.control.is_cancelled());
    let listed = manager.list("root", true).unwrap();
    assert_eq!(
        listed
            .iter()
            .map(|n| n.agent_id.as_str())
            .collect::<Vec<_>>(),
        vec![a.as_str(), b.as_str(), c.as_str()]
    );
    assert_eq!(manager.list("root", false).unwrap().len(), 1);
    manager.shutdown().await;
    assert!(!manager.has_pending("root"));
}

#[tokio::test]
/// 活跃邮箱有界且消息确认可追踪；直接子能给父发送，兄弟不能互聊。
async fn mailbox_limits_and_parent_messages() {
    let parent = root();
    let limits = SubagentLimits {
        max_messages: 2,
        ..Default::default()
    };
    let (manager, mut receiver) = setup(parent.clone(), Arc::new(FakeRepo::default()), limits);
    let child = manager
        .spawn("root", &parent, "任务", "child", false, vec![])
        .await
        .unwrap();
    let _run = receiver.recv().await.unwrap();
    let first = manager.send("root", &child, "one").await.unwrap();
    let second = manager.send("root", &child, "two").await.unwrap();
    assert_ne!(first, second);
    assert!(manager.send("root", &child, "three").await.is_err());
    let messages = manager.drain(&child);
    assert_eq!(messages.len(), 2);
    assert_eq!(messages[0].message_id, first);
    manager.send(&child, "root", "父级请查收").await.unwrap();
    manager.wait("root").await;
    assert_eq!(manager.drain("root")[0].agent_id, child);
    let sibling = manager
        .spawn("root", &parent, "sibling", "sibling", false, vec![])
        .await
        .unwrap();
    let _sibling_run = receiver.recv().await.unwrap();
    assert!(manager.send(&child, &sibling, "越权").await.is_err());
    manager.shutdown().await;
}

#[tokio::test]
/// 冷加载仅靠子文件父关系，保持身份并与本次根授权重新相交。
async fn cold_resume_uses_authoritative_relation_and_scope() {
    let repo = Arc::new(FakeRepo::default());
    let mut old = AgentSession {
        id: "saved".into(),
        parent_session_id: Some("root".into()),
        delegation_depth: 1,
        selected_paths: vec!["a.md".into()],
        ..Default::default()
    };
    old.messages.push(AgentMessage {
        content: "old-history".into(),
        ..Default::default()
    });
    repo.create(&old).unwrap();
    let (manager, mut receiver) = setup(root(), repo.clone(), SubagentLimits::default());
    assert_eq!(manager.list("root", false).unwrap()[0].status, "ready");
    manager.send("root", "saved", "恢复").await.unwrap();
    let run = receiver.recv().await.unwrap();
    assert_eq!(run.execution.session.id, "saved");
    assert_eq!(run.execution.session.selected_paths, vec!["a.md"]);
    assert_eq!(run.execution.session.messages[0].content, "old-history");
    run.reply.send("第一轮".into()).unwrap();
    idle(&manager, "saved").await;
    manager.send("root", "saved", "再续聊").await.unwrap();
    let next = receiver.recv().await.unwrap();
    assert_eq!(next.execution.session.messages.len(), 2);
    old.id = "unrelated".into();
    old.parent_session_id = Some("other".into());
    repo.create(&old).unwrap();
    assert!(manager.send("root", "unrelated", "拒绝").await.is_err());
    old.id = "narrowed".into();
    old.parent_session_id = Some("root".into());
    old.selected_paths = vec!["a.md".into(), "outside.md".into()];
    repo.create(&old).unwrap();
    assert!(manager
        .send("root", "narrowed", "拒绝继承广范围历史")
        .await
        .is_err());
    old.id = "all-vault".into();
    old.selected_paths.clear();
    repo.create(&old).unwrap();
    assert!(manager
        .send("root", "all-vault", "拒绝整库历史缩窄")
        .await
        .is_err());
    old.id = "disjoint".into();
    old.parent_session_id = Some("root".into());
    old.selected_paths = vec!["outside.md".into()];
    repo.create(&old).unwrap();
    assert!(manager
        .send("root", "disjoint", "拒绝空交集")
        .await
        .is_err());
    manager.shutdown().await;
}

#[path = "tests_catalog.rs"]
mod catalog;
#[path = "tests_lifecycle.rs"]
mod lifecycle;
#[path = "tests_models.rs"]
mod models;
#[path = "tests_observation.rs"]
mod observation;
#[path = "tests_receipts.rs"]
mod receipts;
#[path = "tests_terminal.rs"]
mod terminal;
#[path = "tests_writes.rs"]
mod writes;
