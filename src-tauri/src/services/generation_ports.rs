use std::{future::Future, pin::Pin};

use crate::{
    ai::{MultiToolRequest, ToolCallBatch},
    dictionary::{Dictionary, DictionaryEntry},
    error::CommandError,
    models::NoteCard,
};

/// 生成开始时已核对磁盘正文与卡片的不可变快照。
pub(crate) struct GenerationSnapshot {
    pub note_content: String,
    pub cards: Vec<NoteCard>,
}

/// 可取消的异步端口不暴露 HTTP、词典连接或持久化实现。
pub(crate) type PortFuture<'a, T> =
    Pin<Box<dyn Future<Output = Result<T, CommandError>> + Send + 'a>>;

/// 生成执行器只请求协议无关的工具批次。
pub(super) trait GenerationModel: Sync {
    /// 生命周期覆盖单次请求，丢弃 Future 即停止等待网络结果。
    fn call<'a>(&'a self, request: MultiToolRequest<'a>) -> PortFuture<'a, ToolCallBatch>;

    /// 带可见文本增量的调用；默认忽略增量，旧适配器无需改动。
    fn call_streaming<'a>(
        &'a self,
        request: MultiToolRequest<'a>,
        _delta: &'a (dyn Fn(&str) + Send + Sync),
    ) -> PortFuture<'a, ToolCallBatch> {
        self.call(request)
    }
}

/// 生成与 Agent 工具共用的词典只读端口；故障只影响当前工具，不中断调用方流程。
pub(crate) trait DictionaryLookup: Sync {
    /// 未命中返回空值，连接故障通过安全命令错误返回。
    fn lookup<'a>(&'a self, word: &'a str) -> PortFuture<'a, Option<DictionaryEntry>>;
}

impl DictionaryLookup for Dictionary {
    /// 只读词典适配器保持执行器可由内存假实现测试。
    fn lookup<'a>(&'a self, word: &'a str) -> PortFuture<'a, Option<DictionaryEntry>> {
        Box::pin(Dictionary::lookup(self, word))
    }
}
