//! 调试构建的模型流回显：思考、正文与工具调用分段打印到控制台。
//!
//! 只用于本地观察推理内容与工具参数，方便对着实际输出调提示词；release 构建
//! 编译成空实现，不产生任何输出，也不接触凭据。

#[cfg(debug_assertions)]
use std::sync::atomic::{AtomicU8, Ordering};

use super::chunk::StreamChunk;

/// 当前已开启的段落；切换时补换行，避免思考与正文粘连。
#[cfg(debug_assertions)]
const IDLE: u8 = 0;
#[cfg(debug_assertions)]
const REASONING: u8 = 1;
#[cfg(debug_assertions)]
const TEXT: u8 = 2;
#[cfg(debug_assertions)]
static SEGMENT: AtomicU8 = AtomicU8::new(IDLE);

/// 逐字回显一个块；工具参数等非文本块在收尾时单独成行。
pub(crate) fn push(piece: &StreamChunk) {
    #[cfg(debug_assertions)]
    {
        use std::io::Write;
        let (segment, prefix, text) = match piece {
            StreamChunk::ReasoningDelta { text, .. } => (REASONING, "[think] ", text.as_str()),
            StreamChunk::TextDelta { text, .. } => (TEXT, "[text] ", text.as_str()),
            _ => return,
        };
        let previous = SEGMENT.swap(segment, Ordering::Relaxed);
        if previous != segment {
            if previous != IDLE {
                eprintln!();
            }
            eprint!("{prefix}");
        }
        eprint!("{text}");
        let _ = std::io::stderr().flush();
    }
    #[cfg(not(debug_assertions))]
    let _ = piece;
}

/// 请求收尾：结束当前段落，下一条日志不会粘在正文后面。
pub(crate) fn finish() {
    #[cfg(debug_assertions)]
    if SEGMENT.swap(IDLE, Ordering::Relaxed) != IDLE {
        eprintln!();
    }
}

/// 每个工具调用回显为单行，参数保留完整 JSON 便于核对提示词效果。
pub(crate) fn tool_call(name: &str, arguments: &serde_json::Value) {
    #[cfg(debug_assertions)]
    eprintln!("[tool] {name} {arguments}");
    #[cfg(not(debug_assertions))]
    {
        let _ = (name, arguments);
    }
}
