// 当前技术债：本文件已超过 500 行，后续按笔记、卡片、视频场景拆成独立 include 子文件。
use super::*;
use crate::ai::GenerationSession;
use crate::ai::ToolDefinition;
use crate::{
    dictionary::DictionaryEntry,
    models::GenerationInput,
    services::{
        agent_ports::{
            AgentCall, AgentCards, AgentFuture, AgentLearning, AgentModel, AgentModelReply,
            AgentPorts, AgentVideo, PreparedGeneration,
        },
        agent_tasks::AgentTasks,
        generation_ports::{DictionaryLookup, PortFuture},
    },
    storage::{agent::AgentFiles, testutil::TempDir},
};
use std::{collections::VecDeque, sync::Mutex, time::Duration};

#[test]
/// 旧请求不传图片仍可读取，新图片必须进入可恢复历史且不消耗文本预算。
fn image_history_and_validation() {
    let root = TempDir::new();
    let files = AgentFiles::new(root.path()).unwrap();
    let mut session = files.create_session().unwrap();
    let mut input: AgentInput = serde_json::from_value(json!({
        "sessionId": session.id, "requestId":"image-request", "content":"",
        "providerId":"test", "selectedPaths":[]
    }))
    .unwrap();
    assert!(input.images.is_empty());
    input.images.push(crate::models::GenerationImage {
        name: "paste.png".into(),
        mime_type: "image/png".into(),
        data_base64: "aW1hZ2U=".into(),
    });
    assert!(input.validate_images().is_ok());
    begin(&files, &mut session, &input).unwrap();
    let mut restored = files.session(&session.id).unwrap();
    let context = history::context(&mut restored);
    assert_eq!(
        context[0]["content"][1]["image_url"]["url"],
        "data:image/png;base64,aW1hZ2U="
    );
    assert!(history::text_size(&context[0]) < 100);
    input.images[0].data_base64 = "invalid!".into();
    assert!(input.validate_images().is_err());
    input.images.clear();
    assert!(input.validate_images().is_ok());
}

#[test]
/// 分级读取返回带行号的窗口与 nextOffset，hash 供编辑使用。
fn read_note_pages_with_line_numbers_and_hash() {
    let root = TempDir::new();
    let files = AgentFiles::new(root.path()).unwrap();
    let change = files
        .change(
            &uuid::Uuid::now_v7().to_string(),
            "a.md",
            "第一行\n第二行\n第三行",
            None,
        )
        .unwrap();
    let cards = FakeCards::default();
    let context = tools::ToolContext {
        repository: &files,
        learning: &FakeLearning,
        video: &FakeVideo,
        dictionary: &FakeDictionary,
        cards: &cards,
        scope: &[],
        write_scope: crate::services::agent_write_scope::WriteAuthority::Root,
        operation: "read-test",
    };
    let registered = tools::registry();
    let read = registered
        .iter()
        .find(|tool| tool.spec.name == "read_note")
        .expect("read_note 必须注册");
    let first = (read.handler)(&context, &json!({"path":"a.md","limit":2}))
        .unwrap()
        .value;
    assert_eq!(first["totalLines"], 3);
    assert_eq!(first["offset"], 1);
    assert_eq!(first["lines"][0], json!({"number":1,"text":"第一行"}));
    assert_eq!(first["nextOffset"], 3);
    assert_eq!(first["hash"].as_str(), Some(change.after_hash.as_str()));
    let last = (read.handler)(&context, &json!({"path":"a.md","offset":3}))
        .unwrap()
        .value;
    assert_eq!(last["lines"].as_array().unwrap().len(), 1);
    assert!(last["nextOffset"].is_null());
}

struct FakeModel {
    replies: Mutex<VecDeque<AgentModelReply>>,
}

struct InterruptedModel;
impl AgentModel for InterruptedModel {
    /// 在最终响应前模拟流中断，验证已经显示的文字不会从历史消失。
    fn call<'a>(
        &'a self,
        _: &'a str,
        _: &'a [Value],
        _: &'a [ToolDefinition],
        delta: &'a (dyn Fn(&str) + Send + Sync),
        _reasoning: &'a (dyn Fn(&str) + Send + Sync),
    ) -> AgentFuture<'a, AgentModelReply> {
        Box::pin(async move {
            delta("已经输出的文字");
            Err(CommandError::new("AGENT_CANCELLED", "已停止"))
        })
    }
}

struct ReasoningModel;
impl AgentModel for ReasoningModel {
    /// 只发出推理与正文，用于验证推理落盘且排在正文之前。
    fn call<'a>(
        &'a self,
        _: &'a str,
        _: &'a [Value],
        _: &'a [ToolDefinition],
        _delta: &'a (dyn Fn(&str) + Send + Sync),
        reasoning: &'a (dyn Fn(&str) + Send + Sync),
    ) -> AgentFuture<'a, AgentModelReply> {
        Box::pin(async move {
            reasoning("先读材料");
            reasoning("，再作答");
            Ok(AgentModelReply {
                text: "答案".into(),
                ..Default::default()
            })
        })
    }
}

#[tokio::test]
/// 推理持久化并排在正文之前，重新打开会话仍能复盘当时的判断。
async fn persists_reasoning_before_text() {
    let root = TempDir::new();
    let files = AgentFiles::new(root.path()).unwrap();
    let mut session = files.create_session().unwrap();
    let input: AgentInput = serde_json::from_value(json!({
        "sessionId":session.id,"requestId":"reasoning","content":"你好",
        "providerId":"test","selectedPaths":[]
    }))
    .unwrap();
    let tasks = AgentTasks::default();
    let (control, _) = tasks
        .register("main", "root", &input.request_id, &session.id)
        .unwrap();
    let cards = FakeCards::default();
    execute(
        AgentPorts {
            model: &ReasoningModel,
            repository: &files,
            learning: &FakeLearning,
            video: &FakeVideo,
            dictionary: &FakeDictionary,
            cards: &cards,
        },
        &mut session,
        &input,
        &control,
    )
    .await
    .unwrap();
    let persisted = session
        .messages
        .iter()
        .filter(|message| {
            message.role == "assistant" && matches!(message.kind.as_str(), "reasoning" | "text")
        })
        .map(|message| (message.kind.as_str(), message.content.as_str()))
        .collect::<Vec<_>>();
    assert_eq!(
        persisted,
        [("reasoning", "先读材料，再作答"), ("text", "答案")]
    );
    let restored = files.session(&session.id).unwrap();
    assert!(restored
        .messages
        .iter()
        .any(|message| message.kind == "reasoning" && message.content == "先读材料，再作答"));
}

#[tokio::test]
/// 中断后保留部分答案和停止状态，重新打开会话仍能读到。
async fn interrupted_stream_preserves_text() {
    let root = TempDir::new();
    let files = AgentFiles::new(root.path()).unwrap();
    let mut session = files.create_session().unwrap();
    let input: AgentInput = serde_json::from_value(json!({
        "sessionId":session.id,"requestId":"interrupted","content":"你好",
        "providerId":"test","selectedPaths":[]
    }))
    .unwrap();
    let tasks = AgentTasks::default();
    let (control, _) = tasks
        .register("main", "root", &input.request_id, &session.id)
        .unwrap();
    let cards = FakeCards::default();
    assert!(execute(
        AgentPorts {
            model: &InterruptedModel,
            repository: &files,
            learning: &FakeLearning,
            video: &FakeVideo,
            dictionary: &FakeDictionary,
            cards: &cards,
        },
        &mut session,
        &input,
        &control
    )
    .await
    .is_err());
    let restored = files.session(&session.id).unwrap();
    assert_eq!(
        restored
            .messages
            .iter()
            .filter(|message| message.content == "已经输出的文字")
            .count(),
        1
    );
    assert!(restored
        .messages
        .iter()
        .all(|message| message.kind != "running"));
}
impl AgentModel for FakeModel {
    /// 确定性模型让测试只验证用例执行，不发起网络请求。
    fn call<'a>(
        &'a self,
        _: &'a str,
        _: &'a [Value],
        _: &'a [ToolDefinition],
        delta: &'a (dyn Fn(&str) + Send + Sync),
        _reasoning: &'a (dyn Fn(&str) + Send + Sync),
    ) -> AgentFuture<'a, AgentModelReply> {
        Box::pin(async move {
            let reply = self
                .replies
                .lock()
                .unwrap()
                .pop_front()
                .ok_or_else(|| CommandError::validation("测试响应耗尽"))?;
            delta(&reply.text);
            Ok(reply)
        })
    }
}
struct FakeLearning;
impl AgentLearning for FakeLearning {
    /// 拆卡准备只构造内存会话，测试不读磁盘、也不发起模型请求。
    fn prepare<'a>(&'a self, path: &'a str, kind: &'a str) -> AgentFuture<'a, PreparedGeneration> {
        Box::pin(async move {
            let input = GenerationInput {
                type_id: kind.to_string(),
                study_mode_id: if kind == "vocabulary" {
                    "dictation".to_string()
                } else {
                    "self-review".to_string()
                },
                note_title: path.to_string(),
                source_text: "学习材料".to_string(),
                images: vec![],
                requested_count: -1,
                context: None,
            };
            let session = GenerationSession::prepared(&input, input.source_text.clone(), &[]);
            Ok(PreparedGeneration {
                path: path.to_string(),
                kind: kind.to_string(),
                expected_vault_path: "Vault".to_string(),
                expected_note_hash: "hash".to_string(),
                input,
                session,
            })
        })
    }
}
/// 为每个模型调用构造规范化配对记录。
fn reply(name: &str, arguments: Value) -> AgentModelReply {
    AgentModelReply {
        text: String::new(),
        calls: vec![AgentCall {
            id: "call_1".into(),
            name: name.into(),
            arguments: arguments.clone(),
        }],
        replay: json!({"role":"assistant","tool_calls":[{"id":"call_1","type":"function","function":{"name":name,"arguments":arguments.to_string()}}]}),
    }
}
/// 一次响应里包含多个工具调用，用于驱动拆卡模式的三步流程。
fn multi_reply(calls: Vec<AgentCall>) -> AgentModelReply {
    let tool_calls = calls
        .iter()
        .map(|call| {
            json!({"id":call.id,"type":"function","function":{"name":call.name,"arguments":call.arguments.to_string()}})
        })
        .collect::<Vec<_>>();
    AgentModelReply {
        text: String::new(),
        calls,
        replay: json!({"role":"assistant","tool_calls":tool_calls}),
    }
}

/// 在隔离根目录执行，并模拟编辑器完成草稿保存的握手。
async fn execute_fixture(
    files: &AgentFiles,
    scope: Vec<String>,
    replies: Vec<AgentModelReply>,
) -> AgentSession {
    let mut session = files.create_session().unwrap();
    let input = AgentInput {
        session_id: session.id.clone(),
        request_id: uuid::Uuid::now_v7().to_string(),
        content: "请完成任务".into(),
        provider_id: "test".into(),
        selected_paths: scope,
        images: vec![],
    };
    let tasks = AgentTasks::default();
    let (control, _) = tasks
        .register("main", "root", &input.request_id, &session.id)
        .unwrap();
    let model = FakeModel {
        replies: Mutex::new(replies.into()),
    };
    let cards = FakeCards::default();
    let execution = async {
        let result = execute(
            AgentPorts {
                model: &model,
                repository: files,
                learning: &FakeLearning,
                video: &FakeVideo,
                dictionary: &FakeDictionary,
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
            if let Some(id) = control.snapshot().pending_write_id {
                control.acknowledge(&id).await.unwrap();
            } else {
                tokio::time::sleep(Duration::from_millis(1)).await;
            }
        }
    };
    tokio::time::timeout(Duration::from_secs(3), async {
        tokio::join!(execution, editor);
    })
    .await
    .unwrap();
    session
}

#[tokio::test]
/// 连续修改同一路径使用不同握手身份，最终正文和所有工具配对均被保存。
async fn consecutive_writes_to_same_path_do_not_deadlock() {
    let root = TempDir::new();
    let files = AgentFiles::new(root.path()).unwrap();
    let hash = format!("{:x}", <sha2::Sha256 as sha2::Digest>::digest(b"first"));
    let session = execute_fixture(
        &files,
        vec![],
        vec![
            reply("create_note", json!({"path":"a.md","content":"first"})),
            reply(
                "edit_note",
                json!({"path":"a.md","content":"second","expectedHash":hash}),
            ),
            AgentModelReply {
                text: "完成".into(),
                ..Default::default()
            },
        ],
    )
    .await;
    assert_eq!(files.read("a.md").unwrap()["content"], "second");
    assert_eq!(
        session
            .messages
            .iter()
            .filter(|m| m.kind == "change")
            .count(),
        2
    );
    for exchange in session.messages.iter().filter(|m| m.kind == "exchange") {
        assert_eq!(exchange.data["results"].as_array().unwrap().len(), 1);
        assert!(exchange.data["results"][0]["content"]
            .as_str()
            .unwrap()
            .contains("\"ok\":true"));
    }
}

#[tokio::test]
/// 工具范围在后端执行阶段验证，未知工具和范围外写入都不能改变文件。
async fn scope_and_registry_are_enforced() {
    let root = TempDir::new();
    let files = AgentFiles::new(root.path()).unwrap();
    let before = files
        .change(&uuid::Uuid::now_v7().to_string(), "b.md", "protected", None)
        .unwrap();
    let session = execute_fixture(
        &files,
        vec!["a.md".into()],
        vec![
            reply(
                "edit_note",
                json!({"path":"b.md","content":"bad","expectedHash":before.after_hash}),
            ),
            reply("exec_shell", json!({"command":"anything"})),
            AgentModelReply {
                text: "无法访问".into(),
                ..Default::default()
            },
        ],
    )
    .await;
    assert_eq!(files.read("b.md").unwrap()["content"], "protected");
    assert_eq!(
        session
            .messages
            .iter()
            .filter(|m| m.kind == "status")
            .count(),
        2
    );
}

#[tokio::test]
/// Agent 讲解生词可直接查询词典，音标与释义进入工具历史。
async fn dictionary_lookup_reaches_model_history() {
    let root = TempDir::new();
    let files = AgentFiles::new(root.path()).unwrap();
    let session = execute_fixture(
        &files,
        vec![],
        vec![
            reply("lookup_dictionary", json!({"words":["speak"]})),
            AgentModelReply {
                text: "speak 读 /spi:k/".into(),
                ..Default::default()
            },
        ],
    )
    .await;
    let exchange = session
        .messages
        .iter()
        .find(|m| m.kind == "exchange")
        .unwrap();
    let content = exchange.data["results"][0]["content"].as_str().unwrap();
    assert!(content.contains("spi:k"));
    assert!(content.contains("说"));
}

#[tokio::test]
/// 教学纯文字不产生复习，正式复习仅创建等待用户操作的消息块。
async fn review_and_generation_pause_without_implicit_writes() {
    let root = TempDir::new();
    let files = AgentFiles::new(root.path()).unwrap();
    let chat = execute_fixture(
        &files,
        vec![],
        vec![AgentModelReply {
            text: "你如何理解这个概念？".into(),
            ..Default::default()
        }],
    )
    .await;
    assert!(!chat.messages.iter().any(|m| m.kind == "review"));
    let review = execute_fixture(
        &files,
        vec![],
        vec![reply("start_review", json!({"mode":"today","paths":[]}))],
    )
    .await;
    assert!(review.messages.iter().any(|m| m.kind == "review"));
    files
        .change(&uuid::Uuid::now_v7().to_string(), "a.md", "material", None)
        .unwrap();
    // 拆卡已并入 Agent：进入模式后由同一循环执行生成工具，结束时落草稿。
    let generated = execute_fixture(
        &files,
        vec![],
        vec![
            reply("generate_cards", json!({"path":"a.md","kind":"qa"})),
            multi_reply(vec![
                AgentCall {
                    id: "plan_1".into(),
                    name: "plan_cards".into(),
                    arguments: json!({"items":[{"source":"学习材料","keyword":"考点"}]}),
                },
                AgentCall {
                    id: "card_1".into(),
                    name: "emit_card".into(),
                    arguments: json!({"schema_version":1,"type_id":"qa","source":"学习材料","fields":{"front":"问题","back":"答案","detail":""}}),
                },
                AgentCall {
                    id: "finish_1".into(),
                    name: "finish_generation".into(),
                    arguments: json!({"reason":"材料已用尽"}),
                },
            ]),
            AgentModelReply {
                text: "已准备好草稿".into(),
                ..Default::default()
            },
        ],
    )
    .await;
    let drafts = generated
        .messages
        .iter()
        .find(|message| message.kind == "drafts")
        .expect("拆卡结束必须落草稿消息");
    assert_eq!(drafts.data["cards"].as_array().unwrap().len(), 1);
    assert_eq!(drafts.data["path"], "a.md");
}

#[tokio::test]
/// 同一响应里查词成功会推迟落卡，下一轮才接受——词典延迟规则在 Agent 拆卡模式下仍生效。
async fn generation_mode_defers_cards_after_lookup() {
    let root = TempDir::new();
    let files = AgentFiles::new(root.path()).unwrap();
    files
        .change(&uuid::Uuid::now_v7().to_string(), "w.md", "material", None)
        .unwrap();
    let card = json!({"schema_version":1,"type_id":"vocabulary","source":"学习材料","fields":{"front":"v. 说","back":"speak","detail":"","example":"","aliases":""}});
    let session = execute_fixture(
        &files,
        vec![],
        vec![
            reply("generate_cards", json!({"path":"w.md","kind":"vocabulary"})),
            multi_reply(vec![
                AgentCall {
                    id: "plan_1".into(),
                    name: "plan_cards".into(),
                    arguments: json!({"items":[{"source":"学习材料","keyword":"speak"}]}),
                },
                AgentCall {
                    id: "look_1".into(),
                    name: "lookup_words".into(),
                    arguments: json!({"words":["speak"]}),
                },
                AgentCall {
                    id: "card_1".into(),
                    name: "emit_card".into(),
                    arguments: card.clone(),
                },
            ]),
            multi_reply(vec![AgentCall {
                id: "card_2".into(),
                name: "emit_card".into(),
                arguments: card,
            }]),
            multi_reply(vec![AgentCall {
                id: "finish_1".into(),
                name: "finish_generation".into(),
                arguments: json!({"reason":"材料已用尽"}),
            }]),
            AgentModelReply {
                text: "已准备好草稿".into(),
                ..Default::default()
            },
        ],
    )
    .await;
    let drafts = session
        .messages
        .iter()
        .find(|message| message.kind == "drafts")
        .expect("拆卡结束必须落草稿消息");
    assert_eq!(drafts.data["cards"].as_array().unwrap().len(), 1);
    // 同轮查词被推迟的那次调用必须有回执，模型才知道要等下一轮。
    let deferred = session
        .messages
        .iter()
        .filter(|message| message.kind == "exchange")
        .any(|message| {
            message.data["results"].as_array().is_some_and(|results| {
                results.iter().any(|result| {
                    result["content"]
                        .as_str()
                        .unwrap_or("")
                        .contains("LOOKUP_RESULT_PENDING")
                })
            })
        });
    assert!(deferred, "同轮查词后落卡必须被推迟到下一轮");
}

#[tokio::test]
/// 取消等待写入的任务不执行文件动作，状态仍可查询并带安全错误。
async fn cancellation_before_editor_ack_does_not_write() {
    let root = TempDir::new();
    let files = AgentFiles::new(root.path()).unwrap();
    let mut session = files.create_session().unwrap();
    let tasks = AgentTasks::default();
    let (control, _) = tasks.register("main", "root", "run", &session.id).unwrap();
    let input = AgentInput {
        session_id: session.id.clone(),
        request_id: "run".into(),
        content: "create".into(),
        images: vec![],
        provider_id: "test".into(),
        selected_paths: vec![],
    };
    let model = FakeModel {
        replies: Mutex::new(
            vec![reply("create_note", json!({"path":"a.md","content":"no"}))].into(),
        ),
    };
    let cancel = async {
        while control.snapshot().pending_write.is_none() {
            tokio::time::sleep(Duration::from_millis(1)).await;
        }
        control.cancel();
    };
    let cards = FakeCards::default();
    let result = tokio::time::timeout(Duration::from_secs(2), async {
        tokio::join!(
            execute(
                AgentPorts {
                    model: &model,
                    repository: &files,
                    learning: &FakeLearning,
                    video: &FakeVideo,
                    dictionary: &FakeDictionary,
                    cards: &cards,
                },
                &mut session,
                &input,
                &control
            ),
            cancel
        )
    })
    .await
    .unwrap()
    .0;
    assert_eq!(result.err().unwrap().code, "AGENT_CANCELLED");
    assert!(!root.path().join("a.md").exists());
    assert!(session.messages.iter().any(|m| m.kind == "exchange"));
}

/// 测试用词典端口：返回固定词条，不打开真实 ECDICT 文件。
struct FakeDictionary;

impl DictionaryLookup for FakeDictionary {
    /// 固定返回可辨识的音标与释义，验证查询结果进入模型历史。
    fn lookup<'a>(&'a self, word: &'a str) -> PortFuture<'a, Option<DictionaryEntry>> {
        Box::pin(async move {
            Ok(Some(DictionaryEntry {
                word: word.to_string(),
                phonetic: Some("spi:k".into()),
                translation: Some("v. 说, 讲话".into()),
                definition: None,
                pos: Some("v".into()),
                collins: Some(5),
                oxford: Some(1),
                bnc: Some(352),
                frq: Some(335),
                exchange: None,
            }))
        })
    }
}

/// 测试用视频端口：不访问网络，直接返回形状正确的摘要。
struct FakeVideo;

impl AgentVideo for FakeVideo {
    /// 固定返回完成状态，覆盖工具结果写入会话的路径。
    fn run<'a>(
        &'a self,
        url: &'a str,
        note: bool,
    ) -> crate::services::agent_ports::AgentFuture<'a, serde_json::Value> {
        Box::pin(async move {
            let note_path = if note {
                serde_json::Value::String("视频笔记/demo.md".to_string())
            } else {
                serde_json::Value::Null
            };
            Ok(serde_json::json!({
                "state": "completed",
                "output": if note { "note" } else { "transcript" },
                "url": url,
                "segments": 3,
                "shots": 1,
                "transcriptSource": "bilibili_ai",
                "notePath": note_path,
            }))
        })
    }
}

/// 测试用卡片端口：记录删除请求，不接触真实卡片存储。
#[derive(Default)]
struct FakeCards {
    deleted: Mutex<Vec<(String, String)>>,
}

impl AgentCards for FakeCards {
    /// 固定返回一张可识别的卡片摘要，验证结果进入工具历史。
    fn list<'a>(&'a self, note_path: &'a str) -> AgentFuture<'a, Value> {
        Box::pin(async move {
            Ok(json!({
                "path": note_path,
                "total": 1,
                "truncated": false,
                "items": [{"id":"card-1","kind":"qa","front":"问题","back":"答案"}],
            }))
        })
    }

    /// 只记录删除参数；真实归属校验由存储适配器负责。
    fn delete<'a>(&'a self, note_path: &'a str, card_id: &'a str) -> AgentFuture<'a, Value> {
        Box::pin(async move {
            self.deleted
                .lock()
                .unwrap()
                .push((note_path.to_string(), card_id.to_string()));
            Ok(json!({"path": note_path, "id": card_id, "kind": "qa", "front": "问题"}))
        })
    }
}

#[tokio::test]
/// 卡片工具的范围在端口前强制校验，范围外请求不会到达删除端口。
async fn card_tools_enforce_scope_before_port() {
    let root = TempDir::new();
    let files = AgentFiles::new(root.path()).unwrap();
    files
        .change(&uuid::Uuid::now_v7().to_string(), "a.md", "material", None)
        .unwrap();
    let cards = FakeCards::default();
    let scope = vec!["a.md".to_string()];
    let context = tools::ToolContext {
        repository: &files,
        learning: &FakeLearning,
        video: &FakeVideo,
        dictionary: &FakeDictionary,
        cards: &cards,
        scope: &scope,
        write_scope: crate::services::agent_write_scope::WriteAuthority::Root,
        operation: "card-test",
    };
    let registered = tools::registry();
    let list = registered
        .iter()
        .find(|tool| tool.spec.name == "list_cards")
        .expect("list_cards 必须注册");
    let delete = registered
        .iter()
        .find(|tool| tool.spec.name == "delete_card")
        .expect("delete_card 必须注册");
    assert_eq!(delete.spec.schema["required"], json!(["path", "cardId"]));
    assert_eq!(
        delete.spec.effect,
        crate::ai::tools::spec::ToolEffect::Mutate
    );

    let denied_args = json!({"path":"b.md","cardId":"card-1"});
    let denied = (delete.async_handler.unwrap())(&context, &denied_args).await;
    assert_eq!(denied.err().unwrap().code, "AGENT_SCOPE_DENIED");
    assert!(cards.deleted.lock().unwrap().is_empty());

    let list_args = json!({"path":"a.md"});
    let listed = (list.async_handler.unwrap())(&context, &list_args)
        .await
        .unwrap();
    assert_eq!(listed.value["items"][0]["front"], "问题");

    let delete_args = json!({"path":"a.md","cardId":"card-1"});
    let removed = (delete.async_handler.unwrap())(&context, &delete_args)
        .await
        .unwrap();
    assert_eq!(removed.value["id"], "card-1");
    assert_eq!(
        *cards.deleted.lock().unwrap(),
        [("a.md".to_string(), "card-1".to_string())]
    );
    // 删除不可撤销：必须留下用户可见的卡片块，而不是只回给模型。
    let message = removed.message.expect("删除必须留下可见卡片块");
    assert_eq!(message.kind, "card");
    assert_eq!(message.data["state"], "deleted");
    assert_eq!(message.data["front"], "问题");
}
