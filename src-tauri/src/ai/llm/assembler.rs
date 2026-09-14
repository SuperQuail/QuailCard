//! 把 chunk 流折叠成内容块；循环、重放与诊断共用同一实现。

use std::collections::BTreeMap;

use super::chunk::{ContentKind, ReplayEnvelope, StreamChunk};
use super::failure::LlmFailure;
use super::vocabulary::{ContentBlock, FinishReason, TokenUsage};

/// 分段用量合并：后到的非零字段覆盖先前的零值，避免 message_delta 覆盖输入用量。
fn merge_usage(previous: Option<TokenUsage>, next: TokenUsage) -> TokenUsage {
    let Some(previous) = previous else {
        return next;
    };
    TokenUsage {
        input_tokens: if next.input_tokens > 0 {
            next.input_tokens
        } else {
            previous.input_tokens
        },
        output_tokens: if next.output_tokens > 0 {
            next.output_tokens
        } else {
            previous.output_tokens
        },
        total_tokens: next.total_tokens.or(previous.total_tokens),
        cache_read_tokens: next.cache_read_tokens.or(previous.cache_read_tokens),
        cache_write_tokens: next.cache_write_tokens.or(previous.cache_write_tokens),
        reasoning_tokens: next.reasoning_tokens.or(previous.reasoning_tokens),
    }
}

/// 单个块的累积状态。
#[derive(Debug, Clone)]
enum Partial {
    Text(String),
    Reasoning(String),
    ToolCall {
        id: String,
        item_id: Option<String>,
        name: String,
        arguments: String,
    },
    Closed(ContentBlock),
}

/// 已组装的一次工具调用；item_id 只在 Responses 续传时存在。
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct AssembledCall {
    pub id: String,
    pub item_id: Option<String>,
    pub name: String,
    pub arguments: String,
}

/// chunk 流到内容块的唯一折叠实现。
#[derive(Debug, Default)]
pub(crate) struct BlockAssembler {
    partials: BTreeMap<usize, Partial>,
    order: Vec<usize>,
    usage: Option<TokenUsage>,
    finish: Option<(FinishReason, Option<LlmFailure>)>,
    replay: Option<ReplayEnvelope>,
}

impl BlockAssembler {
    /// 消费一个 chunk；同一 index 的重复 block-start 不覆盖已有累积。
    pub(crate) fn push(&mut self, chunk: StreamChunk) {
        match chunk {
            StreamChunk::BlockStart { index, kind } => self.open(index, kind),
            StreamChunk::TextDelta { index, text } => self.append_text(index, &text),
            StreamChunk::ReasoningDelta { index, text } => self.append_reasoning(index, &text),
            StreamChunk::ToolCallDelta {
                index,
                id,
                item_id,
                name,
                arguments_delta,
            } => self.append_tool(index, &id, item_id, name.as_deref(), &arguments_delta),
            StreamChunk::BlockEnd { index, block } => self.close(index, block),
            StreamChunk::Usage(usage) => self.usage = Some(merge_usage(self.usage.take(), usage)),
            StreamChunk::Finish {
                reason,
                failure,
                replay,
            } => {
                self.finish = Some((reason, failure));
                self.replay = replay;
            }
        }
    }

    /// 记录 index 首次出现顺序，保证输出稳定。
    fn touch(&mut self, index: usize) {
        if !self.order.contains(&index) {
            self.order.push(index);
        }
    }

    /// 打开一个块；已存在时保留累积内容。
    fn open(&mut self, index: usize, kind: ContentKind) {
        self.touch(index);
        if self.partials.contains_key(&index) {
            return;
        }
        let partial = match kind {
            ContentKind::Text => Partial::Text(String::new()),
            ContentKind::Reasoning => Partial::Reasoning(String::new()),
            ContentKind::ToolCall => Partial::ToolCall {
                id: String::new(),
                item_id: None,
                name: String::new(),
                arguments: String::new(),
            },
            ContentKind::Image => Partial::Closed(ContentBlock::Image {
                name: String::new(),
                mime: String::new(),
                data_base64: String::new(),
            }),
        };
        self.partials.insert(index, partial);
    }

    /// 追加文本增量。
    fn append_text(&mut self, index: usize, text: &str) {
        self.touch(index);
        if let Partial::Text(buffer) = self
            .partials
            .entry(index)
            .or_insert_with(|| Partial::Text(String::new()))
        {
            buffer.push_str(text);
        }
    }

    /// 追加思考增量；思考与可见文本分开累积。
    fn append_reasoning(&mut self, index: usize, text: &str) {
        self.touch(index);
        if let Partial::Reasoning(buffer) = self
            .partials
            .entry(index)
            .or_insert_with(|| Partial::Reasoning(String::new()))
        {
            buffer.push_str(text);
        }
    }

    /// 追加工具调用增量；id、item_id 与 name 只取首个非空值，参数持续拼接。
    fn append_tool(
        &mut self,
        index: usize,
        id: &str,
        item_id: Option<String>,
        name: Option<&str>,
        arguments: &str,
    ) {
        self.touch(index);
        let partial = self
            .partials
            .entry(index)
            .or_insert_with(|| Partial::ToolCall {
                id: String::new(),
                item_id: None,
                name: String::new(),
                arguments: String::new(),
            });
        if let Partial::ToolCall {
            id: current_id,
            item_id: current_item_id,
            name: current_name,
            arguments: current_arguments,
        } = partial
        {
            if current_id.is_empty() && !id.is_empty() {
                id.clone_into(current_id);
            }
            if current_item_id.is_none() {
                *current_item_id = item_id.filter(|value| !value.is_empty());
            }
            if current_name.is_empty() {
                if let Some(name) = name.filter(|value| !value.is_empty()) {
                    name.clone_into(current_name);
                }
            }
            current_arguments.push_str(arguments);
        }
    }

    /// 用终止块替换累积结果；adapter 的规范块优先。
    fn close(&mut self, index: usize, block: ContentBlock) {
        self.touch(index);
        self.partials.insert(index, Partial::Closed(block));
    }

    /// 已组装内容；max-tokens 截断时丢弃工具调用，因为半截参数不安全。
    pub(crate) fn blocks(&self) -> Vec<ContentBlock> {
        let truncated = matches!(self.finish.as_ref(), Some((FinishReason::MaxTokens, _)));
        self.order
            .iter()
            .filter_map(|index| self.partials.get(index))
            .filter_map(|partial| match partial {
                Partial::Text(text) => Some(ContentBlock::Text { text: text.clone() }),
                Partial::Reasoning(text) => Some(ContentBlock::Reasoning { text: text.clone() }),
                Partial::ToolCall { .. } if truncated => None,
                Partial::ToolCall {
                    id,
                    name,
                    arguments,
                    ..
                } => Some(ContentBlock::ToolCall {
                    id: id.clone(),
                    name: name.clone(),
                    arguments: arguments.clone(),
                }),
                Partial::Closed(block) => Some(block.clone()),
            })
            .collect()
    }

    /// 取消或中断时只保留文本与思考，丢弃半截工具调用；取消路径接入后使用。
    #[allow(dead_code)]
    pub(crate) fn interrupted_blocks(&self) -> Vec<ContentBlock> {
        self.order
            .iter()
            .filter_map(|index| self.partials.get(index))
            .filter_map(|partial| match partial {
                Partial::Text(text) => Some(ContentBlock::Text { text: text.clone() }),
                Partial::Reasoning(text) => Some(ContentBlock::Reasoning { text: text.clone() }),
                Partial::Closed(ContentBlock::Text { text }) => {
                    Some(ContentBlock::Text { text: text.clone() })
                }
                Partial::Closed(ContentBlock::Reasoning { text }) => {
                    Some(ContentBlock::Reasoning { text: text.clone() })
                }
                _ => None,
            })
            .collect()
    }

    /// 已组装的工具调用，按首次出现顺序；供运行时解码成 ToolCallResult。
    pub(crate) fn tool_calls(&self) -> Vec<AssembledCall> {
        // 截断的参数不安全，与 blocks() 保持同一丢弃决策。
        if matches!(self.finish.as_ref(), Some((FinishReason::MaxTokens, _))) {
            return Vec::new();
        }
        self.order
            .iter()
            .filter_map(|index| self.partials.get(index))
            .filter_map(|partial| match partial {
                Partial::ToolCall {
                    id,
                    item_id,
                    name,
                    arguments,
                } => Some(AssembledCall {
                    id: id.clone(),
                    item_id: item_id.clone(),
                    name: name.clone(),
                    arguments: arguments.clone(),
                }),
                _ => None,
            })
            .collect()
    }

    /// 供应商用量，可能缺省。
    pub(crate) fn usage(&self) -> Option<&TokenUsage> {
        self.usage.as_ref()
    }

    /// 终止原因与失败事实。
    pub(crate) fn finish(&self) -> Option<(FinishReason, Option<LlmFailure>)> {
        self.finish.clone()
    }

    /// 重放元数据只在同一 adapter 内使用。
    pub(crate) fn replay(&self) -> Option<&ReplayEnvelope> {
        self.replay.as_ref()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 构造文本增量。
    fn text(index: usize, value: &str) -> StreamChunk {
        StreamChunk::TextDelta {
            index,
            text: value.into(),
        }
    }

    #[test]
    /// 交错的文本与工具参数按 index 折叠，输出顺序稳定。
    fn folds_interleaved_blocks() {
        let mut assembler = BlockAssembler::default();
        assembler.push(StreamChunk::BlockStart {
            index: 0,
            kind: ContentKind::Text,
        });
        assembler.push(StreamChunk::BlockStart {
            index: 1,
            kind: ContentKind::ToolCall,
        });
        assembler.push(text(0, "先看"));
        assembler.push(StreamChunk::ToolCallDelta {
            index: 1,
            id: "call_1".into(),
            item_id: None,
            name: Some("lookup_words".into()),
            arguments_delta: "{\"words\":[".into(),
        });
        assembler.push(text(0, "词典"));
        assembler.push(StreamChunk::ToolCallDelta {
            index: 1,
            id: "call_1".into(),
            item_id: None,
            name: None,
            arguments_delta: "\"speak\"]}".into(),
        });
        assembler.push(StreamChunk::Finish {
            reason: FinishReason::ToolCalls,
            failure: None,
            replay: None,
        });
        let blocks = assembler.blocks();
        assert_eq!(blocks.len(), 2);
        assert_eq!(
            blocks[0],
            ContentBlock::Text {
                text: "先看词典".into()
            }
        );
        assert_eq!(
            blocks[1],
            ContentBlock::ToolCall {
                id: "call_1".into(),
                name: "lookup_words".into(),
                arguments: "{\"words\":[\"speak\"]}".into(),
            }
        );
    }

    #[test]
    /// max-tokens 截断时工具调用被丢弃，文本仍然保留。
    fn drops_tool_calls_on_max_tokens() {
        let mut assembler = BlockAssembler::default();
        assembler.push(text(0, "正文"));
        assembler.push(StreamChunk::ToolCallDelta {
            index: 1,
            id: "c1".into(),
            item_id: None,
            name: Some("emit_card".into()),
            arguments_delta: "{\"schema".into(),
        });
        assembler.push(StreamChunk::Finish {
            reason: FinishReason::MaxTokens,
            failure: None,
            replay: None,
        });
        let blocks = assembler.blocks();
        assert_eq!(blocks.len(), 1);
        assert!(matches!(blocks[0], ContentBlock::Text { .. }));
    }

    #[test]
    /// 取消路径只保留文本与思考，半截工具调用不会执行。
    fn interrupted_keeps_only_text_and_reasoning() {
        let mut assembler = BlockAssembler::default();
        assembler.push(text(0, "已答"));
        assembler.push(StreamChunk::ReasoningDelta {
            index: 1,
            text: "想过".into(),
        });
        assembler.push(StreamChunk::ToolCallDelta {
            index: 2,
            id: "c1".into(),
            item_id: Some("fc_1".into()),
            name: Some("emit_card".into()),
            arguments_delta: "{".into(),
        });
        let blocks = assembler.interrupted_blocks();
        assert_eq!(blocks.len(), 2);
        assert!(blocks
            .iter()
            .all(|block| !matches!(block, ContentBlock::ToolCall { .. })));
    }

    #[test]
    /// BlockEnd 的规范块覆盖累积结果，避免 adapter 与累积器不一致。
    fn block_end_overrides_partial() {
        let mut assembler = BlockAssembler::default();
        assembler.push(text(0, "半截"));
        assembler.push(StreamChunk::BlockEnd {
            index: 0,
            block: ContentBlock::Text {
                text: "完整".into(),
            },
        });
        assert_eq!(
            assembler.blocks(),
            vec![ContentBlock::Text {
                text: "完整".into()
            }]
        );
    }

    #[test]
    /// 用量与终止原因分别记录，互不覆盖。
    fn records_usage_before_finish() {
        let mut assembler = BlockAssembler::default();
        assembler.push(StreamChunk::Usage(TokenUsage {
            input_tokens: 10,
            output_tokens: 4,
            ..Default::default()
        }));
        assembler.push(StreamChunk::Finish {
            reason: FinishReason::Stop,
            failure: None,
            replay: None,
        });
        assert_eq!(assembler.usage().unwrap().input_tokens, 10);
        assert_eq!(assembler.finish().unwrap().0, FinishReason::Stop);
    }
}
