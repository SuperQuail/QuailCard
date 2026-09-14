mod catalog;
mod lifecycle;
mod observation;
pub(crate) use catalog::{stored_children, ChildRecord, ChildSnapshot};
mod messaging;
mod ports;
mod spawn;
mod terminal;
#[cfg(test)]
mod tests;
mod writes;

use crate::{
    agent_models::AgentSession,
    error::CommandError,
    services::{agent_tasks::AgentControl, agent_write_scope},
};
pub(crate) use ports::*;
use std::{
    collections::{BTreeMap, VecDeque},
    sync::{Arc, Mutex, MutexGuard},
};
pub(crate) use terminal::TerminalLease;
use tokio::{
    sync::{Mutex as AsyncMutex, Notify, OwnedSemaphorePermit, Semaphore},
    task::JoinHandle,
};

/// 单次根 execution 拥有的树；持久身份由仓库负责，活动执行不跨根请求复用。
pub(crate) struct Subagents {
    root_id: String,
    root_control: Arc<AgentControl>,
    repo: Arc<dyn ChildRepository>,
    executor: Arc<dyn ChildExecutor>,
    limits: SubagentLimits,
    state: Mutex<Tree>,
    /// 串行化创建/冷加载准入，不在状态锁内访问仓库。
    admission: AsyncMutex<()>,
    shutdown_lock: AsyncMutex<()>,
    models: Arc<Semaphore>,
}

struct Tree {
    closed: bool,
    terminal_seal: Option<String>,
    nodes: BTreeMap<String, Node>,
    jobs: Vec<JoinHandle<()>>,
}

struct Node {
    parent: Option<String>,
    depth: u32,
    description: String,
    scope: Vec<String>,
    /// 该节点的写入授权；根为空表示整库，子节点为空表示只读。
    write_scope: Vec<String>,
    control: Option<Arc<AgentControl>>,
    running: bool,
    resume_after_cancel: bool,
    inbox: VecDeque<ChildNotification>,
    changed: Arc<Notify>,
}

impl Subagents {
    /// 根快照只用作身份和权限边界，绝不经管理器回存覆盖运行中的父历史。
    pub(crate) fn new(
        root: AgentSession,
        root_control: Arc<AgentControl>,
        repo: Arc<dyn ChildRepository>,
        executor: Arc<dyn ChildExecutor>,
        limits: SubagentLimits,
    ) -> Arc<Self> {
        let root_id = root.id.clone();
        let mut node = Node::from_session(&root);
        node.control = Some(root_control.clone());
        let models = Arc::new(Semaphore::new(limits.max_concurrent_models.max(1)));
        Arc::new(Self {
            root_id: root_id.clone(),
            root_control,
            repo,
            executor,
            limits,
            models,
            state: Mutex::new(Tree {
                closed: false,
                terminal_seal: None,
                nodes: BTreeMap::from([(root_id, node)]),
                jobs: vec![],
            }),
            admission: AsyncMutex::new(()),
            shutdown_lock: AsyncMutex::new(()),
        })
    }

    /// 仅包住一次模型请求；工具执行、子任务等待与文件保存必须先释放许可。
    pub(crate) async fn acquire_model(&self) -> Result<OwnedSemaphorePermit, CommandError> {
        self.ensure_open(&self.lock())?;
        // 只限制同时占用的槽位，不限制累计调用或树龄；等待取消由调用方 control 负责。
        let permit = self
            .models
            .clone()
            .acquire_owned()
            .await
            .map_err(|_| stopped())?;
        self.ensure_open(&self.lock())?;
        Ok(permit)
    }

    /// 中毒锁仍保留取消及清理能力；用户内容不能造成永久无法停止。
    fn lock(&self) -> MutexGuard<'_, Tree> {
        self.state.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// 停止标记在每次实际准入复验，不安装竞争消费根取消通知的监听者。
    fn ensure_open(&self, tree: &Tree) -> Result<(), CommandError> {
        if tree.closed || self.root_control.is_cancelled() {
            Err(stopped())
        } else {
            Ok(())
        }
    }

    /// 只有树内身份才能发起控制操作；磁盘记录不能伪装当前 caller。
    fn caller<'a>(&self, tree: &'a Tree, id: &str) -> Result<&'a Node, CommandError> {
        tree.nodes.get(id).ok_or_else(forbidden)
    }

    /// 消息与结果使用同一字节上限，避免结果通知成为无界内存通道。
    fn validate_text(&self, text: &str) -> Result<(), CommandError> {
        if text.trim().is_empty() || text.len() > self.limits.max_message_bytes {
            Err(CommandError::validation("子 Agent 消息为空或超过大小限制"))
        } else {
            Ok(())
        }
    }
}

impl Node {
    /// 只保留当前授权元数据，不缓存可被 executor 更新的完整历史。
    fn from_session(session: &AgentSession) -> Self {
        Self {
            parent: session.parent_session_id.clone(),
            depth: session.delegation_depth,
            description: session.title.clone(),
            scope: session.selected_paths.clone(),
            write_scope: session.write_scope.clone(),
            control: None,
            running: false,
            resume_after_cancel: false,
            inbox: VecDeque::new(),
            changed: Arc::new(Notify::new()),
        }
    }
}

/// 公开错误不包含磁盘路径、仓库内部错误或凭据。
fn forbidden() -> CommandError {
    CommandError::new("SUBAGENT_FORBIDDEN", "无权操作该子 Agent")
}
/// 停止整树后不再准入新消息或模型请求。
fn stopped() -> CommandError {
    CommandError::new("AGENT_CANCELLED", "子 Agent 执行树已停止")
}
/// 稳定会话身份与每次执行、每条消息身份分别生成，避免复用旧结果。
fn id() -> String {
    uuid::Uuid::now_v7().to_string()
}

/// 空范围沿用项目的整库语义；两个非空范围不相交时拒绝，不能误放大为整库。
fn intersect(scope: &[String], parent: &[String]) -> Result<Vec<String>, CommandError> {
    let mut result = if scope.is_empty() {
        parent.to_vec()
    } else if parent.is_empty() {
        scope.to_vec()
    } else {
        scope
            .iter()
            .filter(|p| parent.contains(p))
            .cloned()
            .collect()
    };
    if result.is_empty() && !scope.is_empty() && !parent.is_empty() {
        return Err(forbidden());
    }
    result.sort();
    result.dedup();
    Ok(result)
}

/// 计算子会话可落盘的写入范围：根是整库，子节点必须逐项落在父范围内。
///
/// 空请求表示只读，对任何父级都合法；非空请求只要有一项越权就整体拒绝，
/// 绝不返回部分授权，避免父级误以为子代理拥有它没有的写权限。
fn grant_write_scope(
    requested: &[String],
    parent: &[String],
    parent_unlimited: bool,
) -> Result<Vec<String>, CommandError> {
    let requested = agent_write_scope::parse(requested)?;
    let parent = agent_write_scope::parse(parent)?;
    agent_write_scope::grant(&requested, &parent, parent_unlimited)
}

/// 冷恢复复验：磁盘记录必须仍在父授权内且已经是规范形，旧文件不能放大写权。
fn restore_write_scope(
    stored: &[String],
    parent: &[String],
    parent_unlimited: bool,
) -> Result<Vec<String>, CommandError> {
    let parsed = agent_write_scope::parse(stored)?;
    let effective = agent_write_scope::grant(
        &parsed,
        &agent_write_scope::parse(parent)?,
        parent_unlimited,
    )?;
    if effective != agent_write_scope::canonical(&parsed) {
        return Err(forbidden());
    }
    Ok(effective)
}

/// 集合比较忽略顺序与重复项，但绝不把空整库范围视为有限文件集。
fn same_scope(left: &[String], right: &[String]) -> bool {
    let left: std::collections::BTreeSet<_> = left.iter().collect();
    let right: std::collections::BTreeSet<_> = right.iter().collect();
    left == right
}

/// 限长只截在 UTF-8 字符边界，避免不可信执行器结果令收尾 panic。
fn bounded(mut text: String, max: usize) -> String {
    if text.len() > max {
        let mut end = max;
        while !text.is_char_boundary(end) {
            end -= 1;
        }
        text.truncate(end);
    }
    text
}
