// 子代理写入范围：默认只读、按路径授权的工具策略与处理器复核。
// 执行树、冷恢复与并发锁的用例在同目录 agent_write_scope_tree_tests.rs。
// （本文件被 include! 进 tests 模块，不能用 //! 内层文档注释。）
use super::*;
use super::tools::CHILD_WRITE_TOOLS;
use crate::{
    services::{agent_tasks::AgentTasks, agent_write_scope::WriteAuthority},
    storage::agent::AgentFiles,
};
use std::{
    sync::{
        atomic::{AtomicUsize, Ordering},
        Mutex,
    },
    time::Duration,
};

/// 端口假实现是零大小类型；静态值让处理器级上下文可以合法借用。
pub(super) static LEARNING: FakeLearning = FakeLearning;
pub(super) static VIDEO: FakeVideo = FakeVideo;
pub(super) static DICTIONARY: FakeDictionary = FakeDictionary;

/// 与存储一致的 SHA-256 文本哈希，用来构造期望版本与陈旧版本。
pub(super) fn hash(content: &str) -> String {
    format!(
        "{:x}",
        <sha2::Sha256 as sha2::Digest>::digest(content.as_bytes())
    )
}

/// 协议配对身份由测试指定，便于定位具体工具调用。
pub(super) fn call(id: &str, name: &str, arguments: Value) -> AgentCall {
    AgentCall {
        id: id.into(),
        name: name.into(),
        arguments,
    }
}

/// 交换块里的工具结果文本；断言稳定 code 与安全文案，不依赖内部结构。
pub(super) fn results(session: &AgentSession) -> Vec<String> {
    session
        .messages
        .iter()
        .filter(|message| message.kind == "exchange")
        .flat_map(|message| {
            message.data["results"]
                .as_array()
                .cloned()
                .unwrap_or_default()
        })
        .filter_map(|result| result["content"].as_str().map(str::to_string))
        .collect()
}

/// 记录每轮工具声明，并按批次回放调用；批次用尽后以文本收尾。
pub(super) struct ScopeModel {
    batches: Vec<Vec<AgentCall>>,
    index: AtomicUsize,
    seen: Mutex<Vec<Vec<String>>>,
}
impl ScopeModel {
    /// 每批调用自然占一轮，测试不依赖模型推理。
    pub(super) fn new(batches: Vec<Vec<AgentCall>>) -> Self {
        Self {
            batches,
            index: AtomicUsize::new(0),
            seen: Mutex::new(Vec::new()),
        }
    }
    /// 取回每轮模型实际看到的工具名。
    pub(super) fn seen(&self) -> Vec<Vec<String>> {
        self.seen.lock().unwrap().clone()
    }
}
impl AgentModel for ScopeModel {
    /// 只假造输出；工具策略、授权校验、持久化与并发都由生产实现负责。
    fn call<'a>(
        &'a self,
        _: &'a str,
        _: &'a [Value],
        definitions: &'a [ToolDefinition],
        _: &'a (dyn Fn(&str) + Send + Sync),
        _: &'a (dyn Fn(&str) + Send + Sync),
    ) -> AgentFuture<'a, AgentModelReply> {
        Box::pin(async move {
            self.seen
                .lock()
                .unwrap()
                .push(definitions.iter().map(|d| d.name.to_string()).collect());
            Ok(
                match self.batches.get(self.index.fetch_add(1, Ordering::SeqCst)) {
                    Some(calls) => multi_reply(calls.clone()),
                    None => AgentModelReply {
                        text: "完成".into(),
                        ..Default::default()
                    },
                },
            )
        })
    }
}

/// 在隔离根目录执行一个从磁盘冷恢复的子会话；宿主模拟编辑器完成写前握手。
async fn run_child(
    files: &AgentFiles,
    write_scope: Vec<String>,
    read_scope: Vec<String>,
    batches: Vec<Vec<AgentCall>>,
) -> (AgentSession, Vec<Vec<String>>) {
    let mut stored = files.create_session().unwrap();
    stored.parent_session_id = Some("root".into());
    stored.delegation_depth = 1;
    stored.selected_paths = read_scope;
    stored.write_scope = write_scope;
    files.save_session(&stored).unwrap();
    // 冷恢复：从磁盘读回，确保 writeScope 真正持久化而不是内存残留。
    let mut session = files.session(&stored.id).unwrap();
    let input = AgentInput {
        session_id: session.id.clone(),
        request_id: uuid::Uuid::now_v7().to_string(),
        content: "完成任务".into(),
        provider_id: "test".into(),
        selected_paths: session.selected_paths.clone(),
        images: vec![],
    };
    let (control, _) = AgentTasks::default()
        .register("main", "root", &input.request_id, &session.id)
        .unwrap();
    let model = ScopeModel::new(batches);
    let cards = FakeCards::default();
    let execution = async {
        let result = execute(
            AgentPorts {
                model: &model,
                repository: files,
                learning: &LEARNING,
                video: &VIDEO,
                dictionary: &DICTIONARY,
                cards: &cards,
            },
            &mut session,
            &input,
            &control,
        )
        .await;
        control.complete(result.as_ref().err());
    };
    let editor = async {
        while control.snapshot().state == "running" {
            match control.snapshot().pending_write_id {
                Some(id) => control.acknowledge(&id).await.unwrap(),
                None => tokio::time::sleep(Duration::from_millis(1)).await,
            }
        }
    };
    tokio::time::timeout(Duration::from_secs(10), async {
        tokio::join!(execution, editor);
    })
    .await
    .unwrap();
    let seen = model.seen();
    (session, seen)
}

#[tokio::test]
/// 无 writeScope 的子代理默认只读：声明里没有写工具，直接调用也被处理器拒绝。
async fn read_only_child_cannot_write_notes() {
    let root = TempDir::new();
    let files = AgentFiles::new(root.path()).unwrap();
    let change = files
        .change(
            &uuid::Uuid::now_v7().to_string(),
            "notes/a.md",
            "原文",
            None,
        )
        .unwrap();
    let (session, seen) = run_child(
        &files,
        vec![],
        vec!["notes/a.md".into()],
        vec![vec![
            call(
                "create",
                "create_note",
                json!({"path":"notes/new.md","content":"新"}),
            ),
            call(
                "edit",
                "edit_note",
                json!({"path":"notes/a.md","content":"改","expectedHash":change.after_hash}),
            ),
        ]],
    )
    .await;
    let definitions = &seen[0];
    for name in CHILD_WRITE_TOOLS {
        assert!(
            !definitions.contains(&name.to_string()),
            "{name} 不得出现在只读子代理声明里"
        );
    }
    for name in [
        "search_notes",
        "read_note",
        "lookup_dictionary",
        "list_cards",
    ] {
        assert!(
            definitions.contains(&name.to_string()),
            "{name} 必须始终可用"
        );
    }
    assert!(
        results(&session).join("\n").contains("\"ok\":false"),
        "越权调用必须有失败回执"
    );
    assert_eq!(files.read("notes/a.md").unwrap()["content"], "原文");
    assert!(!root.path().join("notes/new.md").exists());

    // 即使注册表被绕过，处理器本身也必须拒绝空范围的写入。
    let cards = FakeCards::default();
    let context = tools::ToolContext {
        repository: &files,
        learning: &LEARNING,
        video: &VIDEO,
        dictionary: &DICTIONARY,
        cards: &cards,
        scope: &[],
        write_scope: WriteAuthority::Scoped(&[]),
        operation: "write-scope-test",
    };
    let registry = tools::registry();
    let create = registry
        .iter()
        .find(|tool| tool.spec.name == "create_note")
        .unwrap();
    let edit = registry
        .iter()
        .find(|tool| tool.spec.name == "edit_note")
        .unwrap();
    let created = (create.handler)(&context, &json!({"path":"notes/new.md","content":"新"}));
    assert_eq!(created.err().unwrap().code, "AGENT_WRITE_FORBIDDEN");
    let edited = (edit.handler)(
        &context,
        &json!({"path":"notes/a.md","content":"改","expectedHash":change.after_hash}),
    );
    assert_eq!(edited.err().unwrap().code, "AGENT_WRITE_FORBIDDEN");
    assert_eq!(files.read("notes/a.md").unwrap()["content"], "原文");
}

#[tokio::test]
/// 有范围的子代理：目录前缀内可新建，范围内可改，精确文件只读不能新建，范围外一律拒绝。
async fn granted_child_writes_only_inside_scope() {
    let root = TempDir::new();
    let files = AgentFiles::new(root.path()).unwrap();
    let change = files
        .change(
            &uuid::Uuid::now_v7().to_string(),
            "notes/a.md",
            "原文",
            None,
        )
        .unwrap();
    files
        .change(&uuid::Uuid::now_v7().to_string(), "other.md", "别动", None)
        .unwrap();
    let (session, seen) = run_child(
        &files,
        vec!["notes/".into(), "exact.md".into()],
        vec!["notes/a.md".into(), "exact.md".into()],
        vec![vec![
            call(
                "ok-create",
                "create_note",
                json!({"path":"notes/new.md","content":"新笔记"}),
            ),
            call(
                "ok-edit",
                "edit_note",
                json!({"path":"notes/a.md","content":"已改","expectedHash":change.after_hash}),
            ),
            call(
                "bad-edit",
                "edit_note",
                json!({"path":"other.md","content":"越权","expectedHash":hash("别动")}),
            ),
            call(
                "bad-create",
                "create_note",
                json!({"path":"other/new.md","content":"越权"}),
            ),
            call(
                "exact-create",
                "create_note",
                json!({"path":"exact.md","content":"越权"}),
            ),
        ]],
    )
    .await;
    assert!(seen[0].contains(&"create_note".to_string()));
    assert!(seen[0].contains(&"edit_note".to_string()));
    assert_eq!(files.read("notes/new.md").unwrap()["content"], "新笔记");
    assert_eq!(files.read("notes/a.md").unwrap()["content"], "已改");
    assert_eq!(files.read("other.md").unwrap()["content"], "别动");
    assert!(!root.path().join("other/new.md").exists());
    assert!(!root.path().join("exact.md").exists());
    assert!(results(&session)
        .join("\n")
        .contains("AGENT_WRITE_FORBIDDEN"));
}

#[test]
/// 范围随会话持久化并保持 camelCase；旧文件缺字段读作只读而不是整库。
fn write_scope_round_trips_and_legacy_defaults_to_read_only() {
    let root = TempDir::new();
    let files = AgentFiles::new(root.path()).unwrap();
    let mut session = files.create_session().unwrap();
    session.write_scope = vec!["notes/".into()];
    files.save_session(&session).unwrap();
    let restored = files.session(&session.id).unwrap();
    assert_eq!(restored.write_scope, vec!["notes/".to_string()]);
    assert_eq!(
        serde_json::to_value(&restored).unwrap()["writeScope"],
        json!(["notes/"])
    );
    let legacy: AgentSession = serde_json::from_value(json!({
        "formatVersion": 1, "id": session.id, "title": "旧会话", "messages": []
    }))
    .unwrap();
    assert!(legacy.write_scope.is_empty());
    assert_eq!(
        WriteAuthority::Scoped(&legacy.write_scope)
            .check_create("a.md")
            .err()
            .unwrap()
            .code,
        "AGENT_WRITE_FORBIDDEN"
    );
}

#[test]
/// 根只能授权真实存在的路径，且目录前缀按路径分段匹配：`a/` 不覆盖 `ab/x.md`。
fn root_grants_require_existing_targets_and_match_by_segment() {
    let root = TempDir::new();
    let files = AgentFiles::new(root.path()).unwrap();
    for directory in ["a", "ab"] {
        std::fs::create_dir_all(root.path().join(directory)).unwrap();
    }
    files
        .change(&uuid::Uuid::now_v7().to_string(), "a/x.md", "材料", None)
        .unwrap();
    files
        .change(&uuid::Uuid::now_v7().to_string(), "ab/x.md", "材料", None)
        .unwrap();
    let granted = files
        .validate_write_scope(&["a/".into(), "a/x.md".into(), "/".into()])
        .unwrap();
    assert_eq!(
        granted,
        vec!["/".to_string(), "a/".to_string(), "a/x.md".to_string()]
    );
    for denied in [
        vec!["missing.md".to_string()],
        vec!["missing/".to_string()],
        vec!["a".to_string()],
        vec!["../x.md".to_string()],
        vec!["/abs.md".to_string()],
        vec!["a\\x.md".to_string()],
    ] {
        assert!(
            files.validate_write_scope(&denied).is_err(),
            "{denied:?} 必须被拒"
        );
    }
    let scope = vec!["a/".to_string()];
    let authority = WriteAuthority::Scoped(&scope);
    assert!(authority.check_create("a/new.md").is_ok());
    assert!(authority.check_create("a/sub/new.md").is_ok());
    assert!(authority.check_edit("a/x.md").is_ok());
    for denied in ["ab/x.md", "ab/new.md", "a/x.md.bak", "ax.md"] {
        assert_eq!(
            authority.check_edit(denied).err().unwrap().code,
            "AGENT_WRITE_FORBIDDEN",
            "{denied} 不得被 a/ 覆盖"
        );
    }
}
