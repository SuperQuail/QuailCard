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
            AgentPorts, AgentVideo, AgentWordGroups, PreparedGeneration,
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
        word_groups: &TEST_WORD_GROUPS,
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
            word_groups: &TEST_WORD_GROUPS,
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
            word_groups: &TEST_WORD_GROUPS,
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
                word_groups: &TEST_WORD_GROUPS,
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
                    word_groups: &TEST_WORD_GROUPS,
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
        word_groups: &TEST_WORD_GROUPS,
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

/// 单词组端口替身：默认没有已存在文件，借此覆盖"创建/更新"两条路径。
static TEST_WORD_GROUPS: FakeWordGroups = FakeWordGroups {
    existing: Mutex::new(None),
    root: None,
    synced: Mutex::new(Vec::new()),
};

struct FakeWordGroups {
    /// Some 表示文件已存在；新建路径必须为 None。
    existing: Mutex<Option<String>>,
    /// 绑定真实目录时优先读盘，其余场景读 existing。
    root: Option<std::path::PathBuf>,
    /// 同步调用记录：用于断言写入后确实刷新了索引与卡片。
    synced: Mutex<Vec<String>>,
}

impl FakeWordGroups {
    /// 构造独立替身：existing 模拟磁盘上是否已有同名单词组。
    fn with_existing(existing: Option<&str>) -> Self {
        Self {
            existing: Mutex::new(existing.map(str::to_string)),
            root: None,
            synced: Mutex::new(Vec::new()),
        }
    }

    /// 绑定真实临时目录：多步测试里"先创建再更新"能读到刚写入的文件。
    fn for_root(root: &std::path::Path) -> Self {
        Self {
            existing: Mutex::new(None),
            root: Some(root.to_path_buf()),
            synced: Mutex::new(Vec::new()),
        }
    }
}

impl AgentWordGroups for FakeWordGroups {
    fn load<'a>(&'a self, path: &'a str) -> AgentFuture<'a, Option<String>> {
        Box::pin(async move {
            if let Some(root) = &self.root {
                return match std::fs::read_to_string(root.join(path)) {
                    Ok(content) => Ok(Some(content)),
                    Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
                    Err(error) => Err(error.into()),
                };
            }
            Ok(self.existing.lock().unwrap().clone())
        })
    }

    fn sync<'a>(&'a self, _: &'a str, content: &'a str) -> AgentFuture<'a, ()> {
        let content = content.to_string();
        Box::pin(async move {
            self.synced.lock().unwrap().push(content);
            Ok(())
        })
    }
}

/// 构造单词组工具上下文：真实仓库落临时目录，端口替身便于断言同步次数与内容。
fn word_group_context<'a>(
    files: &'a AgentFiles,
    cards: &'a FakeCards,
    word_groups: &'a FakeWordGroups,
    operation: &'a str,
) -> tools::ToolContext<'a> {
    tools::ToolContext {
        repository: files,
        learning: &FakeLearning,
        video: &FakeVideo,
        dictionary: &FakeDictionary,
        cards,
        word_groups,
        scope: &[],
        write_scope: crate::services::agent_write_scope::WriteAuthority::Root,
        operation,
    }
}

#[tokio::test]
/// 创建单词组：后端生成页 id、真实写盘，并把内容交给同步端口物化卡片。
async fn create_word_group_writes_document_and_syncs() {
    let root = TempDir::new();
    let files = AgentFiles::new(root.path()).unwrap();
    let cards = FakeCards::default();
    let word_groups = FakeWordGroups::with_existing(None);
    let operation = uuid::Uuid::now_v7().to_string();
    let context = word_group_context(&files, &cards, &word_groups, &operation);
    let registered = tools::registry();
    let create = registered
        .iter()
        .find(|tool| tool.spec.name == "create_word_group")
        .expect("create_word_group 必须注册");
    assert!(create.spec.writes());
    assert!(create.async_handler.is_some());
    assert_eq!(create.spec.schema["required"], json!(["path", "words"]));

    let args = json!({
        "path": "英语/词汇.qword",
        "words": [
            {"word": "ephemeral", "phonetic": "/ɪˈfemərəl/", "meaning": "短暂的"},
            {"word": "frugal", "meaning": "节俭的"}
        ]
    });
    let outcome = (create.async_handler.unwrap())(&context, &args)
        .await
        .expect("创建成功");
    assert_eq!(outcome.value["pages"], 2);
    assert_eq!(outcome.value["cards"], 2);

    // 真实落盘：目录按路径创建，文档可被领域解析且 id 非空。
    let written = std::fs::read_to_string(root.path().join("英语/词汇.qword")).unwrap();
    let document = crate::word_group::parse(&written).unwrap();
    assert_eq!(document.words.len(), 2);
    assert!(document.words.iter().all(|page| !page.id.is_empty()));
    assert_eq!(document.words[0].meaning, "短暂的");

    // 同步端口拿到同一份内容，用户可见的改动块也留下了。
    let synced = word_groups.synced.lock().unwrap().clone();
    assert_eq!(synced.len(), 1);
    assert_eq!(synced[0], written);
    let message = outcome.message.expect("写入必须留下可见改动块");
    assert_eq!(message.kind, "change");
    assert_eq!(message.data["pages"], 2);
}

#[tokio::test]
/// 同名文件已存在时创建失败：不覆盖用户已有单词组。
async fn create_word_group_rejects_existing_file() {
    let root = TempDir::new();
    let files = AgentFiles::new(root.path()).unwrap();
    let cards = FakeCards::default();
    let word_groups = FakeWordGroups::with_existing(Some("{}"));
    let operation = uuid::Uuid::now_v7().to_string();
    let context = word_group_context(&files, &cards, &word_groups, &operation);
    let create = tools::registry()
        .into_iter()
        .find(|tool| tool.spec.name == "create_word_group")
        .unwrap();
    let args = json!({"path":"词汇.qword","words":[{"word":"ephemeral","meaning":"短暂的"}]});
    let error = (create.async_handler.unwrap())(&context, &args)
        .await
        .err()
        .expect("必须冲突");
    assert_eq!(error.code, "AGENT_NOTE_CONFLICT");
    assert!(!root.path().join("词汇.qword").exists());
}

#[tokio::test]
/// 更新单词组：必须提交 hash，按单词沿用旧 id，未出现的页从文件里移除。
async fn update_word_group_requires_hash_and_keeps_page_ids() {
    let root = TempDir::new();
    let files = AgentFiles::new(root.path()).unwrap();
    let cards = FakeCards::default();
    let word_groups = FakeWordGroups::for_root(root.path());
    let create_operation = uuid::Uuid::now_v7().to_string();
    let create = tools::registry()
        .into_iter()
        .find(|tool| tool.spec.name == "create_word_group")
        .unwrap();
    let created = json!({
        "path": "词汇.qword",
        "words": [
            {"word": "ephemeral", "meaning": "短暂的"},
            {"word": "frugal", "meaning": "节俭的"}
        ]
    });
    (create.async_handler.unwrap())(
        &word_group_context(&files, &cards, &word_groups, &create_operation),
        &created,
    )
    .await
    .unwrap();
    let hash = files.read("词汇.qword").unwrap()["hash"]
        .as_str()
        .unwrap()
        .to_string();
    let before = crate::word_group::parse(
        &std::fs::read_to_string(root.path().join("词汇.qword")).unwrap(),
    )
    .unwrap();
    let ephemeral_id = before.words[0].id.clone();

    let update = tools::registry()
        .into_iter()
        .find(|tool| tool.spec.name == "update_word_group")
        .unwrap();
    let update_operation = uuid::Uuid::now_v7().to_string();
    let context = word_group_context(&files, &cards, &word_groups, &update_operation);
    // 缺 hash：直接拒绝，避免覆盖并发修改。
    let missing = json!({"path":"词汇.qword","words":[{"word":"ephemeral","meaning":"短暂的"}]});
    assert_eq!(
        (update.async_handler.unwrap())(&context, &missing)
            .await
            .err()
            .unwrap()
            .code,
        "VALIDATION_ERROR"
    );
    // 留下其中一页：ephemeral 沿用旧 id，frugal 被删除。
    let args = json!({
        "path": "词汇.qword",
        "expectedHash": hash,
        "words": [{"word": "Ephemeral", "meaning": "短暂的（新）"}]
    });
    let outcome = (update.async_handler.unwrap())(&context, &args)
        .await
        .expect("更新成功");
    assert_eq!(outcome.value["pages"], 1);
    let after = crate::word_group::parse(
        &std::fs::read_to_string(root.path().join("词汇.qword")).unwrap(),
    )
    .unwrap();
    assert_eq!(after.words.len(), 1);
    assert_eq!(after.words[0].id, ephemeral_id);
    assert_eq!(after.words[0].meaning, "短暂的（新）");
}

#[tokio::test]
/// 路径与内容守住边界：.md 不能用单词组工具，单词组也不能用 edit_note。
async fn word_group_tools_enforce_path_and_field_rules() {
    let root = TempDir::new();
    let files = AgentFiles::new(root.path()).unwrap();
    let cards = FakeCards::default();
    let word_groups = FakeWordGroups::with_existing(None);
    let operation = uuid::Uuid::now_v7().to_string();
    let context = word_group_context(&files, &cards, &word_groups, &operation);
    let registry = tools::registry();
    let create = registry
        .iter()
        .find(|tool| tool.spec.name == "create_word_group")
        .unwrap();
    let md_path = json!({"path":"词汇.md","words":[{"word":"ephemeral","meaning":"短暂的"}]});
    assert_eq!(
        (create.async_handler.unwrap())(&context, &md_path)
            .await
            .err()
            .unwrap()
            .code,
        "WORD_GROUP_PATH_INVALID"
    );
    let unknown = json!({"path":"词汇.qword","words":[{"word":"ephemeral","meaning":"短暂的","id":"hack"}]});
    assert_eq!(
        (create.async_handler.unwrap())(&context, &unknown)
            .await
            .err()
            .unwrap()
            .code,
        "WORD_GROUP_INVALID"
    );
    let no_word = json!({"path":"词汇.qword","words":[{"meaning":"短暂的"}]});
    assert_eq!(
        (create.async_handler.unwrap())(&context, &no_word)
            .await
            .err()
            .unwrap()
            .code,
        "WORD_GROUP_INVALID"
    );

    let edit = registry
        .iter()
        .find(|tool| tool.spec.name == "edit_note")
        .unwrap();
    let edit_args = json!({"path":"词汇.qword","content":"{}","expectedHash":"h"});
    assert_eq!(
        (edit.handler)(&context, &edit_args).err().unwrap().code,
        "WORD_GROUP_TOOL_REQUIRED"
    );
}
