//! 编辑器写入握手的组合层；查询与确认都绑定窗口、固定 Vault 与执行树身份。
use crate::{
    error::CommandError,
    services::agent_tasks::{AgentControl, AgentTasks, PendingWrite},
    vaultfs::VaultState,
};
use serde::Serialize;
use std::{path::Path, sync::Arc};
use tauri::Manager;

/// 保存协调只外传执行、会话与笔记路径；写入正文、其余工具参数和结果不外传。
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentPendingWrite {
    pub execution_id: String,
    pub session_id: String,
    pub path: String,
    pub operation_id: String,
}

impl From<PendingWrite> for AgentPendingWrite {
    /// 字段改名只发生在这里，服务层保持自己的命名。
    fn from(write: PendingWrite) -> Self {
        Self {
            execution_id: write.execution_id,
            session_id: write.session_id,
            path: write.path,
            operation_id: write.operation,
        }
    }
}

/// 整树待保存写入；没有执行树时只报告根执行自身的等待。
pub(crate) fn pending(control: &Arc<AgentControl>) -> Vec<AgentPendingWrite> {
    match control.subagents() {
        Some(tree) => tree
            .pending_writes()
            .into_iter()
            .map(AgentPendingWrite::from)
            .collect(),
        None => control
            .pending_write()
            .map(AgentPendingWrite::from)
            .into_iter()
            .collect(),
    }
}

/// 确认只作用于本执行树内的执行身份；外来身份或过期操作一律拒绝。
pub(crate) async fn acknowledge(
    control: &Arc<AgentControl>,
    execution_id: &str,
    operation_id: &str,
) -> Result<(), CommandError> {
    match control.subagents() {
        Some(tree) => tree.acknowledge_write(execution_id, operation_id).await,
        // 树尚未挂载或已经释放：只允许根执行自身的等待继续。
        None if control.is_execution(execution_id) => control.acknowledge(operation_id).await,
        None => Err(inactive()),
    }
}

/// 根执行必须属于当前窗口与固定 Vault，防止跨库或跨窗口代确认写入。
pub(crate) fn root(
    app: &tauri::AppHandle,
    owner: &str,
    run_id: &str,
) -> Result<Arc<AgentControl>, CommandError> {
    let control = app.state::<AgentTasks>().get(owner, run_id)?;
    let current = app.state::<VaultState>().root()?.ok_or_else(forbidden)?;
    if Path::new(&control.root) != current {
        return Err(forbidden());
    }
    Ok(control)
}

/// 权限错误不包含路径、窗口标签或仓库诊断。
fn forbidden() -> CommandError {
    CommandError::new("SUBAGENT_FORBIDDEN", "子 Agent 不属于当前会话或知识库")
}
/// 执行树已释放时不能再隐式恢复执行，只能通过主会话继续。
fn inactive() -> CommandError {
    CommandError::new("SUBAGENT_INACTIVE", "执行树已结束，请通过主会话继续")
}

#[cfg(test)]
#[path = "agent_writes_tests.rs"]
mod tests;
