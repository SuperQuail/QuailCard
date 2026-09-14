//! 版本化只读观察的组合层；查询绝不调用模型、恢复会话或准入工作。
use crate::{
    agent_models::{AgentRun, AgentSession},
    agent_writes::AgentPendingWrite,
    error::CommandError,
    services::agent_tasks::{AgentControl, AgentTasks},
    storage::agent::{AgentFiles, SessionIdentity},
    vaultfs::VaultState,
};
use serde::Serialize;
use std::{
    collections::HashSet,
    path::{Path, PathBuf},
    sync::Arc,
};
use tauri::Manager;

/// 观察响应保留旧会话/运行 DTO；版本仅描述历史，与运行 sequence 独立。
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentObservation {
    /// 实际被观察身份：childId 优先，否则为 sessionId。
    pub session_id: String,
    /// 不透明 v1 版本令牌；缓存驱逐或重启后可保守变化。
    pub revision: String,
    /// null 只表示与 knownRevision 相同，不表示历史被删除。
    pub session: Option<AgentSession>,
    /// 每次采样的当前运行；冷历史/结束后不可用的子运行为 null。
    pub run: Option<AgentRun>,
    /// 根观察附带整树待编辑器保存的写入；子观察不重复父任务的协调状态。
    pub writes: Vec<AgentPendingWrite>,
}

/// 捕获当前根只访问状态锁；规范化与读盘放到阻塞池中。
pub(crate) fn current_root(app: &tauri::AppHandle) -> Result<PathBuf, CommandError> {
    app.state::<VaultState>()
        .root()?
        .ok_or_else(|| CommandError::new("VAULT_NOT_OPEN", "请先打开知识库"))
}

/// 闭包只收到固定 Vault 仓库，排队后切库不会改写任务目标；迟到旧库结果拒绝返回。
pub(crate) async fn read<T: Send + 'static>(
    app: &tauri::AppHandle,
    action: impl FnOnce(&AgentFiles) -> Result<T, CommandError> + Send + 'static,
) -> Result<T, CommandError> {
    read_at(app, current_root(app)?, action).await
}

/// 运行鉴权与排队读取使用同一次根捕获，防止验证旧库后转而读取新库。
pub(crate) async fn read_at<T: Send + 'static>(
    app: &tauri::AppHandle,
    root: PathBuf,
    action: impl FnOnce(&AgentFiles) -> Result<T, CommandError> + Send + 'static,
) -> Result<T, CommandError> {
    let captured = root.clone();
    let result = tauri::async_runtime::spawn_blocking(move || action(&AgentFiles::new(&captured)?))
        .await
        .map_err(|_| CommandError::new("INTERNAL_ERROR", "会话读取任务失败"))?;
    if current_root(app)? != root {
        return Err(forbidden());
    }
    result
}

/// 有效任务必须同时符合窗口、固定 Vault 与根会话身份；未知 ID 不作为授权依据。
pub(crate) fn control(
    tasks: &AgentTasks,
    owner: &str,
    root: &Path,
    session_id: &str,
    run_id: &str,
) -> Result<Option<Arc<AgentControl>>, CommandError> {
    let control = tasks.get_for_observation(owner, run_id)?;
    if let Some(control) = &control {
        if Path::new(&control.root) != root || control.snapshot().session_id != session_id {
            return Err(forbidden());
        }
    }
    Ok(control)
}

/// 根快照先于历史读取，防止 TextCommitted 清除流文字后历史仍为旧版。
pub(crate) async fn observe(
    app: &tauri::AppHandle,
    owner: &str,
    session_id: String,
    run_id: Option<String>,
    child_id: Option<String>,
    known_revision: Option<String>,
) -> Result<AgentObservation, CommandError> {
    let root = current_root(app)?;
    let control = run_id
        .as_deref()
        .map(|id| control(&app.state::<AgentTasks>(), owner, &root, &session_id, id))
        .transpose()?
        .flatten();
    let run = sample_run(
        control.as_ref(),
        &session_id,
        child_id.as_deref(),
        run_id.is_some(),
    )?;
    // 保存协调只属于根观察；子详情不承担父任务的编辑器握手。
    let writes = match (child_id.as_deref(), control.as_ref()) {
        (None, Some(control)) => crate::agent_writes::pending(control),
        _ => Vec::new(),
    };
    read_at(app, root, move |files| {
        authorize_history(files, &session_id, child_id.as_deref())?;
        let target = child_id.as_deref().unwrap_or(&session_id);
        let (revision, session) = files.observe_history(target, known_revision.as_deref())?;
        Ok(AgentObservation {
            session_id: target.into(),
            revision,
            session,
            run,
            writes,
        })
    })
    .await
}

/// 每次重新采样而非历史缓存运行状态；有效根任务必须返回自身快照。
fn sample_run(
    control: Option<&Arc<AgentControl>>,
    session_id: &str,
    child_id: Option<&str>,
    requested: bool,
) -> Result<Option<AgentRun>, CommandError> {
    Ok(match (control, child_id) {
        (Some(control), None) => Some(control.snapshot()),
        (Some(control), Some(child))
            if !control.is_cancelled() && control.snapshot().state == "running" =>
        {
            control
                .subagents()
                .map(|tree| tree.observe_run(session_id, child))
                .transpose()?
                .flatten()
        }
        (None, None) if requested => {
            return Err(CommandError::new("AGENT_RUN_MISSING", "Agent 任务不存在"))
        }
        _ => None,
    })
}

/// 父链鉴权只读取已验证身份缓存，不因每次轮询重新解析根和祖先的全量历史。
pub(crate) fn authorize_history(
    files: &AgentFiles,
    root_id: &str,
    child_id: Option<&str>,
) -> Result<(), CommandError> {
    let root = files.session_identity(root_id)?;
    if root.id != root_id || root.parent.is_some() || root.depth != 0 {
        return Err(forbidden());
    }
    let Some(child_id) = child_id else {
        return Ok(());
    };
    let child = files.session_identity(child_id)?;
    if child.id != child_id {
        return Err(forbidden());
    }
    check_lineage(files, root_id, child)
}

/// 真后代必须具有连续深度与有界无环父链；根自身和其他根下身份不能冒充子会话。
fn check_lineage(
    files: &AgentFiles,
    root_id: &str,
    mut current: SessionIdentity,
) -> Result<(), CommandError> {
    let mut seen = HashSet::new();
    while let Some(parent_id) = current.parent.as_deref() {
        if !seen.insert(current.id.clone()) || seen.len() > 64 {
            return Err(forbidden());
        }
        let parent = files.session_identity(parent_id)?;
        if parent.id != parent_id || parent.depth.checked_add(1) != Some(current.depth) {
            return Err(forbidden());
        }
        if parent.id == root_id {
            return Ok(());
        }
        current = parent;
    }
    Err(forbidden())
}

/// 统一权限错误不包含路径、窗口标签或仓库诊断。
fn forbidden() -> CommandError {
    CommandError::new("SUBAGENT_FORBIDDEN", "子 Agent 不属于当前会话或知识库")
}

#[cfg(test)]
#[path = "agent_observation_tests.rs"]
mod tests;
