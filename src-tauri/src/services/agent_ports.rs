use super::generation_ports::DictionaryLookup;
use crate::{
    agent_models::{AgentChange, AgentMemory, AgentSession},
    ai::{GenerationSession, ToolDefinition},
    error::CommandError,
    models::GenerationInput,
};
use serde_json::Value;
use std::{future::Future, pin::Pin};

pub(crate) type AgentFuture<'a, T> =
    Pin<Box<dyn Future<Output = Result<T, CommandError>> + Send + 'a>>;

/// 一轮 Agent 执行依赖的窄端口集合，避免逐个参数透传和顺序耦合。
pub(crate) struct AgentPorts<'a> {
    pub model: &'a dyn AgentModel,
    pub repository: &'a dyn AgentRepository,
    pub learning: &'a dyn AgentLearning,
    pub video: &'a dyn AgentVideo,
    pub dictionary: &'a dyn DictionaryLookup,
    pub cards: &'a dyn AgentCards,
}

/// 用例只依赖笔记与会话所需的窄接口，不持有 Storage 或文件系统。
pub(crate) trait AgentRepository: Send + Sync {
    /// 读取已校验路径的正文和内容哈希。
    fn read(&self, path: &str) -> Result<Value, CommandError>;
    /// 按显式资料范围列出或检索正文片段。
    fn search(&self, query: &str, scope: &[String]) -> Result<Value, CommandError>;
    /// 在写前复验内容版本，并持久化可撤销的操作记录。
    fn change(
        &self,
        id: &str,
        path: &str,
        content: &str,
        expected: Option<&str>,
    ) -> Result<AgentChange, CommandError>;
    /// 将当前会话完整保存，失败不能伪装成完成。
    fn save_session(&self, session: &AgentSession) -> Result<(), CommandError>;
    /// 获取用户明确保存的记忆。
    fn memory(&self) -> Result<AgentMemory, CommandError>;
    /// 校验父级要授予子代理的写入范围：vaultfs 净化且目标真实存在，返回规范路径。
    ///
    /// 只有根会话能授予新路径；子代理之间的再授权由管理器的子集校验负责。
    fn validate_write_scope(&self, scope: &[String]) -> Result<Vec<String>, CommandError>;
}

/// 协议无关的模型轮次；供应商续传数据仅在后端当前轮次保留。
#[derive(Default)]
pub(crate) struct AgentModelReply {
    pub text: String,
    pub calls: Vec<AgentCall>,
    pub replay: Value,
}

#[derive(Clone)]
pub(crate) struct AgentCall {
    pub id: String,
    pub name: String,
    pub arguments: Value,
}

/// 适配器负责协议历史配对，服务只提供普通消息及工具返回。
pub(crate) trait AgentModel: Send + Sync {
    /// text 增量只包含对用户可见的文字；reasoning 增量只用于实时展示，都不含凭据。
    fn call<'a>(
        &'a self,
        system: &'a str,
        messages: &'a [Value],
        tools: &'a [ToolDefinition],
        delta: &'a (dyn Fn(&str) + Send + Sync),
        reasoning: &'a (dyn Fn(&str) + Send + Sync),
    ) -> AgentFuture<'a, AgentModelReply>;

    /// 调用方生成诊断 ID；不支持追踪的替代实现仍保持原调用契约。
    fn call_traced<'a>(
        &'a self,
        system: &'a str,
        messages: &'a [Value],
        tools: &'a [ToolDefinition],
        delta: &'a (dyn Fn(&str) + Send + Sync),
        reasoning: &'a (dyn Fn(&str) + Send + Sync),
        _trace: &'a str,
    ) -> AgentFuture<'a, AgentModelReply> {
        self.call(system, messages, tools, delta, reasoning)
    }
}

/// 进入拆卡模式的准备结果：已校验的材料身份与可跨多次工具调用持有的生成会话。
///
/// session 带着已去重的草稿与考点清单，由 Agent 循环在生成模式下持有；
/// 采纳时只认这里的笔记身份，磁盘正文变化会被 storage 的校验拦下。
pub(crate) struct PreparedGeneration {
    pub path: String,
    pub kind: String,
    pub expected_vault_path: String,
    pub expected_note_hash: String,
    pub input: GenerationInput,
    pub session: GenerationSession,
}

/// 学习生成与笔记存储分离，草稿只由已有领域生成器校验。
pub(crate) trait AgentLearning: Send + Sync {
    /// 只做准备工作：读取笔记、构造已校验输入并创建生成会话。
    /// 这里不调用模型；真正的拆卡调用由 Agent 循环在同一个 turn 里驱动。
    fn prepare<'a>(&'a self, path: &'a str, kind: &'a str) -> AgentFuture<'a, PreparedGeneration>;
}

/// 卡片管理端口：Agent 只按笔记列出与删除卡片，不接触卡片存储实现。
pub(crate) trait AgentCards: Send + Sync {
    /// 列出指定笔记的卡片安全摘要，供模型在删除前确认身份。
    fn list<'a>(&'a self, note_path: &'a str) -> AgentFuture<'a, Value>;
    /// 删除指定笔记下的一张卡片；实现方必须复核卡片确实属于该笔记。
    fn delete<'a>(&'a self, note_path: &'a str, card_id: &'a str) -> AgentFuture<'a, Value>;
}

/// 视频能力端口：Agent 只提交链接，不接触下载、转写与文件系统细节。
pub(crate) trait AgentVideo: Send + Sync {
    /// 只返回当前归属任务的安全进度文案；不支持观察时保持空。
    fn progress(&self) -> Option<String> {
        None
    }
    /// 运行视频任务；note 为 false 时只取字，返回给模型的是安全摘要。
    fn run<'a>(&'a self, url: &'a str, note: bool) -> AgentFuture<'a, Value>;
    /// 可选参数保持旧适配器兼容，真实视频适配器负责页面与质量校验。
    fn run_options<'a>(
        &'a self,
        url: &'a str,
        note: bool,
        _options: &'a Value,
    ) -> AgentFuture<'a, Value> {
        self.run(url, note)
    }
    /// 按字符分页读完整转录，每次返回有界材料而非无限上下文。
    fn read_transcript<'a>(
        &'a self,
        _task_id: &'a str,
        _offset: usize,
        _limit: usize,
    ) -> AgentFuture<'a, Value> {
        Box::pin(async { Err(CommandError::validation("当前适配器不支持读取转录")) })
    }
    /// 抽取指定秒数的画面；实现方负责归属校验与图片能力检查。
    ///
    /// 模型据此判断这帧是否可用，不满意就换秒数再调一次。
    fn shot<'a>(&'a self, _task_id: &'a str, _at: f64) -> AgentFuture<'a, Value> {
        Box::pin(async { Err(CommandError::validation("当前适配器不支持抽取画面")) })
    }
}
