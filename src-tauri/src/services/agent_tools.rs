use crate::ai::tools::spec::{ToolEffect, ToolSpec};
use crate::ai::tools::validate;
use crate::ai::ToolDefinition;
use crate::services::agent_ports::{
    AgentCards, AgentFuture, AgentLearning, AgentRepository, AgentVideo, PreparedGeneration,
};
use crate::services::agent_write_scope::WriteAuthority;
use crate::services::generation_ports::DictionaryLookup;
use crate::{agent_models::AgentMessage, error::CommandError};
use serde_json::{json, Value};

#[path = "agent_handlers.rs"]
mod handlers;
#[path = "agent_video_tools.rs"]
mod video_options;
use video_options::{
    note_tool, read_transcript_tool, shot_tool, transcript_tool, validate_video_args, video_schema,
    video_sync,
};

type Handler = fn(&ToolContext<'_>, &Value) -> Result<ToolOutcome, CommandError>;
type AsyncHandler = for<'a, 'b> fn(&'a ToolContext<'b>, &'a Value) -> AgentFuture<'a, ToolOutcome>;
pub(super) struct RegisteredTool {
    pub spec: ToolSpec,
    pub handler: Handler,
    pub async_handler: Option<AsyncHandler>,
}
pub(super) struct ToolContext<'a> {
    pub repository: &'a dyn AgentRepository,
    pub learning: &'a dyn AgentLearning,
    /// 视频能力：Agent 只提交链接，不接触下载与转写细节。
    pub video: &'a dyn AgentVideo,
    /// 词典查询：Agent 只提交词条，校验、截断与占位规则复用生成流程。
    pub dictionary: &'a dyn DictionaryLookup,
    /// 卡片管理：Agent 只按笔记列出与删除卡片。
    pub cards: &'a dyn AgentCards,
    pub scope: &'a [String],
    /// 本会话的笔记写入授权：根为整库，子代理只拥有父级授予的路径。
    pub write_scope: WriteAuthority<'a>,
    pub operation: &'a str,
}
#[derive(Default)]
pub(super) struct ToolOutcome {
    pub value: Value,
    pub message: Option<AgentMessage>,
    pub pause: bool,
    /// 拆卡模式准备结果；由 Agent 循环安装成生成上下文并切换到生成工具集。
    pub generation: Option<PreparedGeneration>,
    /// 需要回给模型的画面；图片只在本轮历史里传输，不写进会话文件。
    pub images: Vec<ToolImage>,
}

/// 工具结果里的图片；Base64 是回合内数据，不进入持久化与会话摘录。
#[derive(Debug, Clone, PartialEq)]
pub(super) struct ToolImage {
    pub mime: String,
    pub data_base64: String,
}

impl ToolImage {
    /// 历史消息使用 data URL，adapter 会把它还原成供应商的图片块。
    pub(super) fn data_url(&self) -> String {
        format!("data:{};base64,{}", self.mime, self.data_base64)
    }
}

/// 工具结果消息：有画面时使用内容块数组，否则保持纯文本字符串。
pub(super) fn tool_message(call_id: &str, value: &Value, images: &[ToolImage]) -> Value {
    let text = value.to_string();
    if images.is_empty() {
        return json!({"role": "tool", "tool_call_id": call_id, "content": text});
    }
    let mut content = vec![json!({"type": "text", "text": text})];
    content.extend(
        images
            .iter()
            .map(|image| json!({"type": "image_url", "image_url": {"url": image.data_url()}})),
    );
    json!({"role": "tool", "tool_call_id": call_id, "content": content})
}

/// 工具清单与执行器共用注册表，模型不能构造未注册动作。
pub(super) fn registry() -> Vec<RegisteredTool> {
    vec![
        tool(
            "search_notes",
            "按关键词检索当前允许范围的笔记；空关键词列出笔记",
            json!({"query":{"type":"string"}}),
            &["query"],
            false,
            handlers::search,
        ),
        tool(
            "read_note",
            "读取 Markdown 的分页窗口（带行号与 hash）；用返回的 nextOffset 继续读到结尾，编辑前必须完整读取",
            json!({"path":{"type":"string"},"offset":{"type":"integer","minimum":1},"limit":{"type":"integer","minimum":1,"maximum":2000}}),
            &["path"],
            false,
            handlers::read,
        ),
        tool(
            "create_note",
            "创建新 Markdown 笔记，不能覆盖同名文件",
            json!({"path":{"type":"string"},"content":{"type":"string"}}),
            &["path", "content"],
            true,
            handlers::create,
        ),
        tool(
            "edit_note",
            "以完整新正文修改笔记，必须提交刚读取的 hash",
            json!({"path":{"type":"string"},"content":{"type":"string"},"expectedHash":{"type":"string"}}),
            &["path", "content", "expectedHash"],
            true,
            handlers::edit,
        ),
        tool(
            "start_review",
            "展示正式复习卡并等待用户作答；today 为到期复习，notes 为指定笔记全部卡片",
            json!({"mode":{"type":"string","enum":["today","notes"]},"paths":{"type":"array","items":{"type":"string"}}}),
            &["mode", "paths"],
            false,
            handlers::review,
        ),
        tool(
            "generate_cards",
            "进入拆卡模式：从指定笔记准备学习卡片草稿，用户选择后采纳；单词选 vocabulary，问答选 qa",
            json!({"path":{"type":"string"},"kind":{"type":"string","enum":["qa","vocabulary"]}}),
            &["path", "kind"],
            false,
            handlers::generate_sync,
        )
        .with_async(handlers::enter_generation),
        tool(
            "lookup_dictionary",
            "查询内置英汉词典的真实音标、释义、词频与词形变化；讲解生词或整理词表前使用",
            json!({"words":{"type":"array","items":{"type":"string"}}}),
            &["words"],
            false,
            handlers::dictionary_sync,
        )
        .with_async(handlers::lookup_dictionary),
        tool(
            "list_cards",
            "列出指定笔记的卡片（id、类型、正反面摘要）；删除前先用它确认 cardId",
            json!({"path":{"type":"string"}}),
            &["path"],
            false,
            handlers::list_cards_sync,
        )
        .with_async(handlers::list_cards),
        tool(
            "delete_card",
            "删除指定笔记下的一张卡片，cardId 必须来自 list_cards；删除不可撤销，仅在用户明确要求时调用",
            json!({"path":{"type":"string"},"cardId":{"type":"string"}}),
            &["path", "cardId"],
            false,
            handlers::delete_card_sync,
        )
        .with_effect(ToolEffect::Mutate)
        .with_async(handlers::delete_card),
        tool(
            "video_transcript",
            "把 B 站视频链接转成带时间轴的转录（字幕模式）：优先读平台字幕，无字幕时本地转写；返回摘要供你自行整理",
            video_schema(),
            &["url"],
            false,
            video_sync,
        )
        .with_async(transcript_tool),
        tool(
            "video_note",
            "把 B 站视频链接整理成结构化笔记并保存到知识库：按内容逻辑重建信息，正文不带时间轴，可按需配关键画面",
            video_schema(),
            &["url"],
            false,
            video_sync,
        )
        .with_async(note_tool),
        tool(
            "video_shot",
            "抽取视频任务里某一秒的画面给你查看：配图前先看这张图，若是转场、黑屏、模糊或与所写内容不符，就换一个秒数再取；返回的 path 可直接写进 Markdown 图片语法",
            json!({"taskId":{"type":"string"},"at":{"type":"number","minimum":0},"reason":{"type":"string"}}),
            &["taskId", "at"],
            false,
            video_sync,
        )
        .with_async(shot_tool),
        tool("video_transcript_read", "按字符偏移分页读取完整视频转录；使用返回的 nextOffset 继续，按需读取避免占满上下文",
            json!({"taskId":{"type":"string"},"offset":{"type":"integer","minimum":0},"limit":{"type":"integer","minimum":1,"maximum":12000}}),
            &["taskId", "offset", "limit"], false, video_sync).with_async(read_transcript_tool),
        tool(
            "remember",
            "仅在用户明确要求记住时提出长期记忆建议，由用户保存",
            json!({"content":{"type":"string"}}),
            &["content"],
            false,
            handlers::remember,
        ),
    ]
}

/// 子代理始终可用的只读业务工具；运行时工具（计划、派生）由注册表自行过滤。
pub(super) const CHILD_READ_TOOLS: &[&str] = &[
    "search_notes",
    "read_note",
    "lookup_dictionary",
    "list_cards",
];

/// 子代理唯一允许的写工具集合；只有父级显式授予 writeScope 时才出现。
pub(super) const CHILD_WRITE_TOOLS: &[&str] = &["create_note", "edit_note"];

/// 子代理工具策略：默认只读，不再依赖调用点维护名字白名单。
///
/// 安全底线：按 ToolSpec.effect 判定——任何写工具必须同时出现在写允许集合里，
/// 新增写工具默认对子代理不可用；Mutate/External（删卡、视频、记忆等）一律不授予。
pub(super) fn child_policy(
    registered: Vec<RegisteredTool>,
    write_scope: &[String],
) -> Vec<RegisteredTool> {
    let writable = !write_scope.is_empty();
    registered
        .into_iter()
        .filter(|tool| match tool.spec.effect {
            ToolEffect::Write => writable && CHILD_WRITE_TOOLS.contains(&tool.spec.name),
            ToolEffect::Read => CHILD_READ_TOOLS.contains(&tool.spec.name),
            ToolEffect::Mutate | ToolEffect::External => false,
        })
        .collect()
}

/// 最小 Schema 保持输入明确，执行前另行校验类型、范围及长度。
fn tool(
    name: &'static str,
    description: &'static str,
    properties: Value,
    required: &[&str],
    write: bool,
    handler: Handler,
) -> RegisteredTool {
    RegisteredTool {
        spec: ToolSpec {
            name,
            description,
            schema: json!({"type":"object","properties":properties,"required":required,"additionalProperties":false}),
            effect: if write {
                ToolEffect::Write
            } else {
                ToolEffect::Read
            },
        },
        handler,
        async_handler: None,
    }
}

impl RegisteredTool {
    /// 外部能力使用异步端口，普通文件工具保持同步原子提交。
    fn with_async(mut self, handler: AsyncHandler) -> Self {
        self.async_handler = Some(handler);
        self
    }

    /// 派生数据修改（如删除卡片）不触发笔记编辑器握手，也不产生 change 记录。
    fn with_effect(mut self, effect: ToolEffect) -> Self {
        self.spec.effect = effect;
        self
    }
}

/// 注册项只经 ToolSpec 产出统一声明，模型看到的工具集与可执行工具集同源。
pub(super) fn definitions(tools: &[RegisteredTool]) -> Vec<ToolDefinition> {
    tools.iter().map(|tool| tool.spec.definition()).collect()
}

/// 参数对象只能包含注册字段，所有必填字段必须存在。
pub(super) fn validate(tool: &RegisteredTool, args: &Value) -> Result<(), CommandError> {
    validate::validate_arguments(&tool.spec, args)?;
    let object = args
        .as_object()
        .expect("validate_arguments 已确认参数是对象");
    let properties = tool.spec.schema["properties"]
        .as_object()
        .expect("注册 Schema 必须有属性");
    if tool.spec.name.starts_with("video_") {
        return validate_video_args(args);
    }
    for (key, value) in object {
        let schema = &properties[key];
        let valid = (schema["type"] == "string" && value.is_string())
            || (schema["type"] == "integer" && value.is_u64())
            || (schema["type"] == "array"
                && value.as_array().is_some_and(|v| {
                    v.len() <= 50
                        && (v.iter().all(Value::is_string) || v.iter().all(Value::is_object))
                }));
        if !valid
            || schema["enum"]
                .as_array()
                .is_some_and(|values| !values.contains(value))
        {
            return Err(CommandError::validation("工具参数类型或选项无效"));
        }
    }
    Ok(())
}

/// 每个交互块有稳定 UUID，可作为复习与采纳的幂等身份。
pub(super) fn block(kind: &str, content: &str, data: Value) -> AgentMessage {
    AgentMessage {
        id: uuid::Uuid::now_v7().to_string(),
        role: "assistant".into(),
        kind: kind.into(),
        content: content.into(),
        data,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    /// 画面工具必须注册且必填 taskId 与 at，否则模型只能盲配图。
    fn video_shot_tool_is_registered() {
        let tool = registry()
            .into_iter()
            .find(|tool| tool.spec.name == "video_shot")
            .expect("video_shot 必须注册");
        assert_eq!(tool.spec.schema["required"], json!(["taskId", "at"]));
        assert!(tool.async_handler.is_some());
        assert!(validate(&tool, &json!({"taskId":"t","at":12.5})).is_ok());
        assert!(validate(&tool, &json!({"taskId":"t"})).is_err());
        assert!(validate(&tool, &json!({"taskId":"t","at":"12"})).is_err());
    }

    #[test]
    /// 文本结果保持字符串，带图结果用内容块数组，图片按 data URL 发送。
    fn tool_message_carries_images_as_content_blocks() {
        let value = json!({"path": "shot.jpg"});
        assert!(tool_message("c1", &value, &[])["content"].is_string());
        let message = tool_message(
            "c1",
            &value,
            &[ToolImage {
                mime: "image/jpeg".into(),
                data_base64: "AAA".into(),
            }],
        );
        assert_eq!(message["content"][0]["type"], "text");
        assert_eq!(
            message["content"][1]["image_url"]["url"],
            "data:image/jpeg;base64,AAA"
        );
    }
}
