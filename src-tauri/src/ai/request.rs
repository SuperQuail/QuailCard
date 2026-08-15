use serde_json::{json, Value};

use super::{MultiToolRequest, ToolDefinition, ToolMessage, ToolRequest};
use crate::models::{GenerationImage, ProviderConfig};

const TOOL_RETRY_INSTRUCTION: &str =
    "上一轮响应结束时没有调用任何工具。本轮禁止使用普通文本结束，必须调用一个可用工具。";

/// 构造 OpenAI Compatible 的多工具流式请求体。
pub(super) fn openai_multi_body(
    config: &ProviderConfig,
    request: &MultiToolRequest<'_>,
    retry_missing_tool: bool,
) -> Value {
    let user_content = openai_user_content(request.user_prompt, request.images);
    let mut messages = vec![
        json!({
            "role": "system",
            "content": system_prompt(request.system_prompt, retry_missing_tool)
        }),
        json!({ "role": "user", "content": user_content }),
    ];
    messages.extend(openai_history_messages(request.history));
    json!({
        "model": config.model,
        "messages": messages,
        "tools": request.tools.iter().map(openai_tool_json).collect::<Vec<_>>(),
        "parallel_tool_calls": false,
        "max_tokens": request.max_tokens,
        "temperature": 0.2,
        "stream": true
    })
}

/// 构造 Anthropic Messages 的多工具流式请求体。
pub(super) fn anthropic_multi_body(
    config: &ProviderConfig,
    request: &MultiToolRequest<'_>,
    retry_missing_tool: bool,
) -> Value {
    let user_content = anthropic_user_content(request.user_prompt, request.images);
    let mut messages = vec![json!({ "role": "user", "content": user_content })];
    messages.extend(anthropic_history_messages(request.history));
    json!({
        "model": config.model,
        "system": system_prompt(request.system_prompt, retry_missing_tool),
        "messages": messages,
        "tools": request.tools.iter().map(anthropic_tool_json).collect::<Vec<_>>(),
        "max_tokens": request.max_tokens,
        "temperature": 0.2,
        "stream": true
    })
}

/// 构造 ChatGPT Codex Responses 的多工具流式请求体。
pub(super) fn openai_responses_multi_body(
    config: &ProviderConfig,
    request: &MultiToolRequest<'_>,
    retry_missing_tool: bool,
) -> Value {
    let content = openai_responses_content(request.user_prompt, request.images);
    let mut input = vec![json!({ "role": "user", "content": content })];
    input.extend(responses_history_items(request.history));
    json!({
        "model": config.model,
        "instructions": system_prompt(request.system_prompt, retry_missing_tool),
        "input": input,
        "tools": request.tools.iter().map(responses_tool_json).collect::<Vec<_>>(),
        "parallel_tool_calls": false,
        "store": false,
        "stream": true
    })
}

/// 构造 OpenAI Compatible 单工具流式请求体。
pub(super) fn openai_body(
    config: &ProviderConfig,
    request: &ToolRequest<'_>,
    retry_missing_tool: bool,
) -> Value {
    let user_content = openai_user_content(request.user_prompt, request.images);
    json!({
        "model": config.model,
        "messages": [
            {
                "role": "system",
                "content": system_prompt(request.system_prompt, retry_missing_tool)
            },
            { "role": "user", "content": user_content }
        ],
        "tools": [openai_tool_json(request.tool)],
        "parallel_tool_calls": false,
        "max_tokens": request.max_tokens,
        "temperature": 0.2,
        "stream": true
    })
}

/// 构造 ChatGPT Codex Responses 单工具流式请求体。
pub(super) fn openai_responses_body(
    config: &ProviderConfig,
    request: &ToolRequest<'_>,
    retry_missing_tool: bool,
) -> Value {
    let content = openai_responses_content(request.user_prompt, request.images);
    json!({
        "model": config.model,
        "instructions": system_prompt(request.system_prompt, retry_missing_tool),
        "input": [{
            "role": "user",
            "content": content
        }],
        "tools": [responses_tool_json(request.tool)],
        "tool_choice": {
            "type": "function",
            "name": request.tool.name
        },
        "parallel_tool_calls": false,
        "store": false,
        "stream": true
    })
}

/// 构造 Anthropic Messages 单工具流式请求体。
pub(super) fn anthropic_body(
    config: &ProviderConfig,
    request: &ToolRequest<'_>,
    retry_missing_tool: bool,
) -> Value {
    let user_content = anthropic_user_content(request.user_prompt, request.images);
    json!({
        "model": config.model,
        "system": system_prompt(request.system_prompt, retry_missing_tool),
        "messages": [{ "role": "user", "content": user_content }],
        "tools": [anthropic_tool_json(request.tool)],
        "tool_choice": {
            "type": "tool",
            "name": request.tool.name,
            "disable_parallel_tool_use": true
        },
        "max_tokens": request.max_tokens,
        "temperature": 0.2,
        "stream": true
    })
}

/// 在工具缺失重试时强化系统提示，不改变供应商的 tool_choice 参数。
fn system_prompt(original: &str, retry_missing_tool: bool) -> String {
    if retry_missing_tool {
        format!("{original}\n\n{TOOL_RETRY_INSTRUCTION}")
    } else {
        original.to_string()
    }
}

/// 将协议无关的调用历史转换为 OpenAI Chat 消息。
fn openai_history_messages(history: &[ToolMessage]) -> Vec<Value> {
    let mut messages = Vec::new();
    let mut pending_calls = Vec::new();
    for message in history {
        match message {
            ToolMessage::AssistantCall {
                id,
                name,
                arguments,
                ..
            } => {
                pending_calls.push(json!({
                    "id": id,
                    "type": "function",
                    "function": { "name": name, "arguments": arguments.to_string() }
                }));
            }
            ToolMessage::ToolResult { id, content } => {
                if !pending_calls.is_empty() {
                    messages.push(json!({
                        "role": "assistant",
                        "content": null,
                        "tool_calls": pending_calls
                    }));
                    pending_calls = Vec::new();
                }
                messages.push(json!({
                    "role": "tool",
                    "tool_call_id": id,
                    "content": content
                }));
            }
            ToolMessage::ProviderItem { .. } => {}
        }
    }
    messages
}

/// 将协议无关的调用历史转换为 Anthropic 消息。
fn anthropic_history_messages(history: &[ToolMessage]) -> Vec<Value> {
    let mut messages = Vec::new();
    let mut pending_calls = Vec::new();
    for message in history {
        match message {
            ToolMessage::AssistantCall {
                id,
                name,
                arguments,
                ..
            } => {
                pending_calls.push(json!({
                    "type": "tool_use",
                    "id": id,
                    "name": name,
                    "input": arguments
                }));
            }
            ToolMessage::ToolResult { id, content } => {
                if !pending_calls.is_empty() {
                    messages.push(json!({
                        "role": "assistant",
                        "content": pending_calls
                    }));
                    pending_calls = Vec::new();
                }
                messages.push(json!({
                    "role": "user",
                    "content": [{ "type": "tool_result", "tool_use_id": id, "content": content }]
                }));
            }
            ToolMessage::ProviderItem { .. } => {}
        }
    }
    messages
}

/// 将协议无关的调用历史转换为 OpenAI Responses 输入项。
fn responses_history_items(history: &[ToolMessage]) -> Vec<Value> {
    let mut items = Vec::new();
    for message in history {
        match message {
            ToolMessage::AssistantCall {
                id,
                item_id,
                name,
                arguments,
            } => {
                items.push(json!({
                    "type": "function_call",
                    "id": item_id.as_deref().unwrap_or(id),
                    "call_id": id,
                    "name": name,
                    "arguments": arguments.to_string()
                }));
            }
            ToolMessage::ToolResult { id, content } => {
                items.push(json!({
                    "type": "function_call_output",
                    "call_id": id,
                    "output": content
                }));
            }
            ToolMessage::ProviderItem { value } => items.push(value.clone()),
        }
    }
    items
}

/// 将单个工具定义转换为 OpenAI Chat 工具 JSON。
fn openai_tool_json(tool: &ToolDefinition) -> Value {
    json!({
        "type": "function",
        "function": {
            "name": tool.name,
            "description": tool.description,
            "strict": true,
            "parameters": tool.input_schema
        }
    })
}

/// 将单个工具定义转换为 Anthropic 工具 JSON。
fn anthropic_tool_json(tool: &ToolDefinition) -> Value {
    json!({
        "name": tool.name,
        "description": tool.description,
        "input_schema": tool.input_schema
    })
}

/// 将单个工具定义转换为 OpenAI Responses 工具 JSON。
fn responses_tool_json(tool: &ToolDefinition) -> Value {
    json!({
        "type": "function",
        "name": tool.name,
        "description": tool.description,
        "parameters": tool.input_schema,
        "strict": true
    })
}

/// 构造 OpenAI Chat Completions 的文字与图片消息内容。
fn openai_user_content(user_prompt: &str, images: &[GenerationImage]) -> Value {
    if images.is_empty() {
        return Value::String(user_prompt.to_string());
    }
    let mut content = vec![json!({ "type": "text", "text": user_prompt })];
    content.extend(images.iter().map(|image| {
        json!({
            "type": "image_url",
            "image_url": { "url": image_data_url(image) }
        })
    }));
    Value::Array(content)
}

/// 构造 OpenAI Responses 的 input_text 与 input_image 内容。
fn openai_responses_content(user_prompt: &str, images: &[GenerationImage]) -> Vec<Value> {
    let mut content = vec![json!({ "type": "input_text", "text": user_prompt })];
    content.extend(
        images
            .iter()
            .map(|image| json!({ "type": "input_image", "image_url": image_data_url(image) })),
    );
    content
}

/// 构造 Anthropic Messages 的文字与 Base64 图片内容。
fn anthropic_user_content(user_prompt: &str, images: &[GenerationImage]) -> Value {
    if images.is_empty() {
        return Value::String(user_prompt.to_string());
    }
    let mut content = vec![json!({ "type": "text", "text": user_prompt })];
    content.extend(images.iter().map(|image| {
        json!({
            "type": "image",
            "source": {
                "type": "base64",
                "media_type": image.mime_type,
                "data": image.data_base64
            }
        })
    }));
    Value::Array(content)
}

/// 将已校验的图片编码拼接为模型接口接受的 Data URL。
fn image_data_url(image: &GenerationImage) -> String {
    format!("data:{};base64,{}", image.mime_type, image.data_base64)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    /// Responses 历史使用 item_id 标识调用项，并用 call_id 关联工具输出。
    fn preserves_responses_item_and_call_ids() {
        let history = vec![
            ToolMessage::AssistantCall {
                id: "call_1".to_string(),
                item_id: Some("fc_1".to_string()),
                name: "lookup_words".to_string(),
                arguments: json!({ "words": ["speak"] }),
            },
            ToolMessage::ToolResult {
                id: "call_1".to_string(),
                content: "result".to_string(),
            },
        ];
        let items = responses_history_items(&history);
        assert_eq!(items[0]["id"], "fc_1");
        assert_eq!(items[0]["call_id"], "call_1");
        assert_eq!(items[1]["call_id"], "call_1");
    }

    #[test]
    /// Responses 原始 reasoning 项会按原顺序回放到下一轮 input。
    fn replays_responses_provider_items() {
        let history = vec![
            ToolMessage::ProviderItem {
                value: json!({
                    "id": "rs_1",
                    "type": "reasoning",
                    "encrypted_content": "encrypted",
                    "summary": []
                }),
            },
            ToolMessage::ProviderItem {
                value: json!({
                    "id": "fc_1",
                    "call_id": "call_1",
                    "type": "function_call",
                    "name": "lookup_words",
                    "arguments": "{\"words\":[\"speak\"]}"
                }),
            },
            ToolMessage::ToolResult {
                id: "call_1".to_string(),
                content: "result".to_string(),
            },
        ];
        let items = responses_history_items(&history);
        assert_eq!(items[0]["type"], "reasoning");
        assert_eq!(items[1]["call_id"], "call_1");
        assert_eq!(items[2]["type"], "function_call_output");
    }
}
