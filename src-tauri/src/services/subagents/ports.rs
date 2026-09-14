use super::{ChildSnapshot, Subagents};
use crate::{agent_models::AgentSession, error::CommandError, services::agent_tasks::AgentControl};
use serde::Serialize;
use std::{future::Future, pin::Pin, sync::Arc};

pub(crate) type ChildFuture =
    Pin<Box<dyn Future<Output = Result<String, CommandError>> + Send + 'static>>;

/// 仓库绑定当前 Vault/所有者；父关系必须来自子文件，不能回写旧父快照。
pub(crate) trait ChildRepository: Send + Sync {
    /// 原子创建全新身份，已存在时拒绝；只保存子会话。
    fn create(&self, child: &AgentSession) -> Result<(), CommandError>;
    /// 加载最新子会话；管理器随后复验父关系与范围。
    fn load(&self, id: &str) -> Result<AgentSession, CommandError>;
    /// 按子文件内的 parent_session_id 查询，不能信任父 children 缓存。
    fn list(&self, parent_id: &str) -> Result<Vec<AgentSession>, CommandError>;
    /// 文件仓库一次扫描生成本次刷新快照；旧假仓库默认回退到逐父查询，无需改实现。
    fn snapshot(&self) -> Result<Option<ChildSnapshot>, CommandError> {
        Ok(None)
    }
}

/// 组合根拥有全部适配器；future 不得借用父轮次的端口或栈帧。
pub(crate) trait ChildExecutor: Send + Sync {
    /// 自行持久化子轮次终态并返回安全摘要；只在模型调用期间持有模型许可。
    fn execute(
        &self,
        execution: ChildExecution,
        manager: Arc<Subagents>,
        control: Arc<AgentControl>,
    ) -> ChildFuture;
}

pub(crate) struct ChildExecution {
    pub session: AgentSession,
    pub prompt: String,
    pub description: String,
    pub execution_id: String,
    pub message_id: String,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ChildNotification {
    pub message_id: String,
    pub agent_id: String,
    pub execution_id: String,
    /// message / completed / failed / cancelled；摘要不是独立验收结论。
    pub kind: String,
    pub content: String,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ChildInfo {
    pub agent_id: String,
    pub parent_session_id: String,
    pub delegation_depth: u32,
    pub description: String,
    /// 父级授予的写入范围；空表示只读，父可据此追溯授权。
    pub write_scope: Vec<String>,
    /// running / idle / ready，磁盘身份 ready 不意味着有结果可收取。
    pub status: String,
}

#[derive(Clone)]
pub(crate) struct SubagentLimits {
    pub max_depth: u32,
    /// 根以外的驻留身份总数；空闲身份也计数，防止派生绕过配额。
    pub max_agents: usize,
    pub max_concurrent_models: usize,
    pub max_messages: usize,
    pub max_message_bytes: usize,
}

impl Default for SubagentLimits {
    /// 默认采用浅树及有界邮箱，不把父等待占作模型请求。
    fn default() -> Self {
        Self {
            max_depth: 3,
            max_agents: 12,
            max_concurrent_models: 4,
            max_messages: 32,
            max_message_bytes: 16 * 1024,
        }
    }
}
