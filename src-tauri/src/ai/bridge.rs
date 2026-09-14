//! 旧请求类型到中立模型请求的桥接：只做数据结构转换，不含网络与存储。

use serde_json::{json, Value};

use super::llm::chunk::ReplayEnvelope;
use super::llm::request::{AssistantTurn, ModelRequest, ToolSchema};
use super::llm::vocabulary::{ContentBlock, Message, MessageSource, Role};
use super::{MultiToolRequest, ToolCallResult, ToolDefinition, ToolMessage, ToolRequest};
use crate::error::CommandError;
use crate::models::{GenerationImage, ProviderConfig};
use crate::services::agent_ports::{AgentCall, AgentModelReply};

/// 工具缺失重试时追加的系统提示；由调用方组合，不改供应商参数。
const TOOL_RETRY_INSTRUCTION: &str =
    "上一轮响应结束时没有调用任何工具。本轮禁止使用普通文本结束，必须调用一个可用工具。";

/// 工具缺失重试时强化系统提示，不改变供应商的 tool_choice 参数。
pub(crate) fn strengthen_system_prompt(original: &str, retry_missing_tool: bool) -> String {
    if retry_missing_tool {
        format!("{original}\n\n{TOOL_RETRY_INSTRUCTION}")
    } else {
        original.to_string()
    }
}

/// 由供应商配置与认证类型构造已验证路由；session 兼作 OpenCode 会话身份。
pub(crate) fn route(
    config: &ProviderConfig,
    auth_type: &'static str,
    max_tokens: Option<u32>,
    temperature: Option<f32>,
    parallel_tool_calls: bool,
    idle_timeout_ms: u64,
    session_id: &str,
) -> Result<super::llm::request::ModelRoute, CommandError> {
    Ok(super::llm::request::ModelRoute {
        provider_id: config.id.clone(),
        protocol: super::ProviderProtocol::parse(&config.protocol)?,
        auth_type,
        model: config.model.clone(),
        base_url: config.base_url.clone(),
        max_tokens,
        temperature,
        parallel_tool_calls,
        idle_timeout_ms,
        session_id: session_id.to_string(),
    })
}

/// 单工具请求转中立模型请求；重试时追加“必须调用工具”提示。
pub(crate) fn single_request(request: &ToolRequest<'_>, retry_missing_tool: bool) -> ModelRequest {
    ModelRequest {
        system: strengthen_system_prompt(request.system_prompt, retry_missing_tool),
        messages: vec![user_message(request.user_prompt, request.images)],
        tools: vec![tool_schema(request.tool)],
        tool_choice: Some(request.tool.name.to_string()),
    }
}

/// 多工具请求转中立模型请求，历史逐条转换。
pub(crate) fn multi_request(
    request: &MultiToolRequest<'_>,
    retry_missing_tool: bool,
) -> ModelRequest {
    let mut messages = vec![user_message(request.user_prompt, request.images)];
    messages.extend(history_messages(request.history));
    ModelRequest {
        system: strengthen_system_prompt(request.system_prompt, retry_missing_tool),
        messages,
        tools: request.tools.iter().map(tool_schema).collect(),
        tool_choice: None,
    }
}

/// 旧工具定义转模型线格式；schema 克隆，名字与描述拥有所有权。
fn tool_schema(definition: &ToolDefinition) -> ToolSchema {
    ToolSchema {
        name: definition.name.to_string(),
        description: definition.description.to_string(),
        parameters: definition.input_schema.clone(),
    }
}

/// 用户消息携带文本与图片块。
fn user_message(prompt: &str, images: &[GenerationImage]) -> Message {
    let mut blocks = vec![ContentBlock::Text {
        text: prompt.to_string(),
    }];
    blocks.extend(images.iter().map(|image| ContentBlock::Image {
        name: image.name.clone(),
        mime: image.mime_type.clone(),
        data_base64: image.data_base64.clone(),
    }));
    Message {
        id: "user".into(),
        role: Role::User,
        blocks,
        source: Some(MessageSource::User),
        replay: None,
    }
}

/// 工具历史转中立消息：调用进 assistant，结果进 user，重放项作为 adapter 私有数据。
fn history_messages(history: &[ToolMessage]) -> Vec<Message> {
    history
        .iter()
        .enumerate()
        .map(|(index, message)| match message {
            ToolMessage::AssistantCall {
                id,
                name,
                arguments,
                ..
            } => Message {
                id: format!("assistant_{index}"),
                role: Role::Assistant,
                blocks: vec![ContentBlock::ToolCall {
                    id: id.clone(),
                    name: name.clone(),
                    arguments: arguments.replay_value().to_string(),
                }],
                source: Some(MessageSource::Model),
                replay: None,
            },
            ToolMessage::ToolResult { id, content } => Message {
                id: format!("tool_{index}"),
                role: Role::User,
                blocks: vec![ContentBlock::ToolResult {
                    call_id: id.clone(),
                    blocks: vec![ContentBlock::Text {
                        text: content.clone(),
                    }],
                    is_error: false,
                }],
                source: Some(MessageSource::Tool),
                replay: None,
            },
            ToolMessage::ProviderItem { value } => Message {
                id: format!("item_{index}"),
                role: Role::Assistant,
                blocks: Vec::new(),
                source: Some(MessageSource::Model),
                replay: Some(json!([value])),
            },
        })
        .collect()
}

/// 从调用列表中取出指定工具参数；缺参数或调错工具都给出安全错误。
pub(crate) fn select_expected_call(
    calls: Vec<ToolCallResult>,
    expected_tool: &str,
) -> Result<Value, CommandError> {
    calls
        .into_iter()
        .find(|call| call.name == expected_tool)
        .map(|call| {
            call.arguments
                .valid()
                .cloned()
                .ok_or_else(invalid_tool_arguments)
        })
        .transpose()?
        .ok_or_else(|| {
            CommandError::provider(
                "PROVIDER_TOOL_RESPONSE_INVALID",
                format!("模型调用了非预期工具，必须调用 {expected_tool}"),
            )
        })
}

/// Responses 无状态续传项；其他协议为空。
pub(crate) fn continuation_items(replay: Option<&ReplayEnvelope>) -> Vec<Value> {
    replay
        .and_then(|envelope| envelope.response.as_ref())
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default()
}

/// 模型参数不是有效 JSON。
fn invalid_tool_arguments() -> CommandError {
    CommandError::provider(
        "PROVIDER_TOOL_RESPONSE_INVALID",
        "模型返回的工具参数不是有效 JSON",
    )
}

/// Agent 的 system/消息/工具转中立请求；历史里无法识别的条目直接丢弃。
pub(crate) fn agent_request(
    system: &str,
    messages: &[Value],
    tools: &[ToolDefinition],
) -> ModelRequest {
    ModelRequest {
        system: system.to_string(),
        messages: agent_messages(messages),
        tools: tools.iter().map(tool_schema).collect(),
        tool_choice: None,
    }
}

/// Agent 历史（OpenAI 形状）转中立消息；Responses 重放项按 adapter 私有数据处理。
pub(crate) fn agent_messages(messages: &[Value]) -> Vec<Message> {
    messages
        .iter()
        .enumerate()
        .filter_map(|(index, value)| agent_message(index, value))
        .collect()
}

/// 单条历史消息；空块与未知角色不产生消息。
fn agent_message(index: usize, value: &Value) -> Option<Message> {
    if let Some(items) = value.get("responseItems").and_then(Value::as_array) {
        return Some(Message {
            id: format!("items_{index}"),
            role: Role::Assistant,
            blocks: Vec::new(),
            source: Some(MessageSource::Model),
            replay: Some(Value::Array(items.clone())),
        });
    }
    let role = match value.get("role").and_then(Value::as_str)? {
        "assistant" => Role::Assistant,
        "tool" | "user" => Role::User,
        _ => return None,
    };
    let mut blocks = Vec::new();
    match role {
        Role::Assistant => {
            if let Some(text) = value.get("content").and_then(Value::as_str) {
                if !text.is_empty() {
                    blocks.push(ContentBlock::Text {
                        text: text.to_string(),
                    });
                }
            }
            for call in value
                .get("tool_calls")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
            {
                blocks.push(ContentBlock::ToolCall {
                    id: call
                        .get("id")
                        .and_then(Value::as_str)
                        .unwrap_or_default()
                        .to_string(),
                    name: call
                        .pointer("/function/name")
                        .and_then(Value::as_str)
                        .unwrap_or_default()
                        .to_string(),
                    arguments: call
                        .pointer("/function/arguments")
                        .and_then(Value::as_str)
                        .unwrap_or_default()
                        .to_string(),
                });
            }
        }
        Role::User => {
            if let Some(call_id) = value.get("tool_call_id").and_then(Value::as_str) {
                // 工具结果可以是纯文本，也可以是 text/image_url 块数组；
                // 图片只有走内容数组才能回给模型做画面复查。
                let mut content = Vec::new();
                collect_content(value.get("content"), &mut content);
                blocks.push(ContentBlock::ToolResult {
                    call_id: call_id.to_string(),
                    blocks: content,
                    is_error: false,
                });
            } else {
                collect_content(value.get("content"), &mut blocks);
            }
        }
        Role::System => return None,
    }
    if blocks.is_empty() {
        return None;
    }
    Some(Message {
        id: format!("m{index}"),
        role,
        blocks,
        source: None,
        replay: None,
    })
}

/// 用户内容支持纯文本与 text/image_url 块数组；data URL 还原成图片块。
fn collect_content(content: Option<&Value>, blocks: &mut Vec<ContentBlock>) {
    match content {
        Some(Value::String(text)) => {
            if !text.is_empty() {
                blocks.push(ContentBlock::Text { text: text.clone() });
            }
        }
        Some(Value::Array(items)) => {
            for item in items {
                if let Some(text) = item.get("text").and_then(Value::as_str) {
                    if !text.is_empty() {
                        blocks.push(ContentBlock::Text {
                            text: text.to_string(),
                        });
                    }
                }
                if let Some(url) = item.pointer("/image_url/url").and_then(Value::as_str) {
                    if let Some((mime, data)) = data_url_parts(url) {
                        blocks.push(ContentBlock::Image {
                            name: String::new(),
                            mime,
                            data_base64: data,
                        });
                    }
                }
            }
        }
        _ => {}
    }
}

/// data URL 拆成 mime 与 base64；非 data URL 忽略。
fn data_url_parts(url: &str) -> Option<(String, String)> {
    let rest = url.strip_prefix("data:")?;
    let (mime, data) = rest.split_once(";base64,")?;
    Some((mime.to_string(), data.to_string()))
}

/// 中立结果还原 Agent 回复；Responses 用原始输出项重放，其余用 assistant 消息。
pub(crate) fn agent_reply(turn: AssistantTurn, responses: bool) -> AgentModelReply {
    let calls: Vec<AgentCall> = turn
        .calls
        .iter()
        .map(|call| AgentCall {
            id: call.id.clone(),
            name: call.name.clone(),
            arguments: call.arguments.valid().cloned().unwrap_or(Value::Null),
        })
        .collect();
    let replay = if responses {
        let items = turn
            .replay
            .as_ref()
            .and_then(|envelope| envelope.response.as_ref())
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();
        json!({"responseItems": items})
    } else {
        json!({
            "role": "assistant",
            "content": turn.text.clone(),
            "tool_calls": calls
                .iter()
                .map(|call| json!({
                    "id": call.id,
                    "type": "function",
                    "function": {"name": call.name, "arguments": call.arguments.to_string()}
                }))
                .collect::<Vec<_>>()
        })
    };
    AgentModelReply {
        text: turn.text,
        calls,
        replay,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ai::llm::vocabulary::FinishReason;
    use crate::ai::{ProviderProtocol, ToolArguments, ToolDefinition};

    #[test]
    /// Agent 历史按角色转换；工具结果与 Responses 重放项各归其位。
    fn agent_history_converts_tools_results_and_replay() {
        let messages = vec![
            json!({"role":"user","content":"读笔记"}),
            json!({"role":"assistant","content":"","tool_calls":[{"id":"c1","type":"function","function":{"name":"read_note","arguments":"{\"path\":\"a.md\"}"}}]}),
            json!({"role":"tool","tool_call_id":"c1","content":"正文"}),
            json!({"responseItems":[{"type":"reasoning","id":"rs_1"}]}),
        ];
        let converted = agent_messages(&messages);
        assert_eq!(converted.len(), 4);
        assert!(matches!(
            converted[1].blocks[0],
            ContentBlock::ToolCall { .. }
        ));
        assert!(matches!(
            converted[2].blocks[0],
            ContentBlock::ToolResult { .. }
        ));
        assert!(converted[3].replay.is_some());
        assert!(agent_message(0, &json!({"role":"system","content":"x"})).is_none());
    }

    #[test]
    /// 工具结果的内容块数组还原成文本与图片块；旧纯文本形式继续可用。
    fn agent_tool_result_keeps_image_blocks() {
        let converted = agent_messages(&[
            json!({
                "role": "tool",
                "tool_call_id": "c1",
                "content": [
                    {"type": "text", "text": "已截帧"},
                    {"type": "image_url", "image_url": {"url": "data:image/jpeg;base64,AAA"}}
                ]
            }),
            json!({"role":"tool","tool_call_id":"c2","content":"纯文本"}),
        ]);
        match &converted[0].blocks[0] {
            ContentBlock::ToolResult { blocks, .. } => {
                assert_eq!(blocks.len(), 2);
                assert!(matches!(blocks[1], ContentBlock::Image { .. }));
            }
            other => panic!("期望工具结果，得到 {other:?}"),
        }
        match &converted[1].blocks[0] {
            ContentBlock::ToolResult { blocks, .. } => {
                assert_eq!(blocks.len(), 1);
                assert!(matches!(blocks[0], ContentBlock::Text { .. }));
            }
            other => panic!("期望工具结果，得到 {other:?}"),
        }
    }

    #[test]
    /// Agent 工具声明直接来自统一 ToolDefinition，不再经过 OpenAI JSON 往返。
    fn agent_request_maps_shared_tool_definitions() {
        let tools = vec![ToolDefinition {
            name: "emit",
            description: "d",
            input_schema: json!({"type": "object", "properties": {}}),
        }];
        let request = agent_request("s", &[json!({"role":"user","content":"x"})], &tools);
        assert_eq!(request.tools.len(), 1);
        assert_eq!(request.tools[0].name, "emit");
        assert_eq!(request.tools[0].description, "d");
        assert_eq!(request.tools[0].parameters["type"], "object");
    }

    #[test]
    /// Responses 与 Chat 的重放形状不同，但文本与调用一致。
    fn agent_reply_shapes_protocol_replay() {
        let turn = AssistantTurn {
            text: "答案".into(),
            blocks: Vec::new(),
            calls: Vec::new(),
            usage: None,
            finish: FinishReason::Stop,
            failure: None,
            replay: None,
        };
        let chat = agent_reply(turn.clone(), false);
        assert_eq!(chat.replay["role"], "assistant");
        assert_eq!(chat.replay["content"], "答案");
        let responses = agent_reply(turn, true);
        assert!(responses.replay["responseItems"].is_array());
    }

    fn config() -> ProviderConfig {
        ProviderConfig {
            id: "provider".into(),
            protocol: "OpenAI Compatible".into(),
            model: "test".into(),
            base_url: "https://example.com/v1".into(),
            secret_ref: None,
            auth_type: Some("api_key".into()),
            oauth_account_id: None,
            provider_type: "api".into(),
            supports_vision: false,
            models: Vec::new(),
        }
    }

    fn tool() -> ToolDefinition {
        ToolDefinition {
            name: "emit",
            description: "d",
            input_schema: json!({"type": "object"}),
        }
    }

    #[test]
    /// 重试时系统提示追加必须调用工具的指令，tool_choice 指向目标工具。
    fn single_request_strengthens_prompt_on_retry() {
        let request = ToolRequest {
            trace_id: "trace",
            turn: 1,
            system_prompt: "系统",
            user_prompt: "用户",
            images: &[],
            tool: &tool(),
            max_tokens: 100,
            timeout: std::time::Duration::from_secs(5),
        };
        let first = single_request(&request, false);
        assert_eq!(first.system, "系统");
        assert_eq!(first.tool_choice.as_deref(), Some("emit"));
        let retry = single_request(&request, true);
        assert!(retry.system.contains("必须调用一个可用工具"));
    }

    #[test]
    /// 历史消息按调用、结果、重放项分别转换。
    fn history_preserves_calls_results_and_replay() {
        let history = vec![
            ToolMessage::AssistantCall {
                id: "call_1".into(),
                item_id: Some("fc_1".into()),
                name: "emit".into(),
                arguments: ToolArguments::Valid(json!({"a": 1})),
            },
            ToolMessage::ToolResult {
                id: "call_1".into(),
                content: "{\"ok\":true}".into(),
            },
            ToolMessage::ProviderItem {
                value: json!({"type": "reasoning", "id": "rs_1"}),
            },
        ];
        let messages = history_messages(&history);
        assert_eq!(messages.len(), 3);
        assert!(matches!(messages[0].role, Role::Assistant));
        assert!(matches!(
            messages[0].blocks[0],
            ContentBlock::ToolCall { .. }
        ));
        assert!(matches!(messages[1].role, Role::User));
        assert!(matches!(
            messages[1].blocks[0],
            ContentBlock::ToolResult { .. }
        ));
        assert!(messages[2].replay.is_some());
    }

    #[test]
    /// 调错工具与无效参数都返回安全错误，不泄漏原文。
    fn select_expected_call_is_strict() {
        let ok = select_expected_call(
            vec![ToolCallResult {
                id: "c1".into(),
                item_id: None,
                name: "emit".into(),
                arguments: ToolArguments::Valid(json!({"a": 1})),
            }],
            "emit",
        )
        .unwrap();
        assert_eq!(ok["a"], 1);
        let wrong = select_expected_call(
            vec![ToolCallResult {
                id: "c1".into(),
                item_id: None,
                name: "other".into(),
                arguments: ToolArguments::Valid(json!({})),
            }],
            "emit",
        )
        .unwrap_err();
        assert_eq!(wrong.code, "PROVIDER_TOOL_RESPONSE_INVALID");
    }

    #[test]
    /// 路由解析协议枚举；Responses 续传项从重放信封提取。
    fn route_and_continuation_contract() {
        let route = route(&config(), "api_key", Some(10), Some(0.2), true, 5000, "s").unwrap();
        assert_eq!(route.protocol, ProviderProtocol::OpenAiCompatible);
        let envelope = ReplayEnvelope {
            response: Some(json!([{"type": "reasoning"}])),
            blocks: Vec::new(),
        };
        assert_eq!(continuation_items(Some(&envelope)).len(), 1);
        assert!(continuation_items(None).is_empty());
    }
}
