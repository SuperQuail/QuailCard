//! UI 自主执行控制的组合层；会话、窗口与固定知识库边界在委派前核实。
use crate::{
    agent_models::AgentSession,
    error::CommandError,
    services::{
        agent_tasks::{AgentControl, AgentTasks},
        subagents::ChildInfo,
    },
    storage::agent::AgentFiles,
    vaultfs::VaultState,
};
use std::sync::Arc;
use tauri::Manager;

/// 根身份不得是子会话；普通 UI 不能把子身份重新提升为根权限。
fn root(files: &AgentFiles, id: &str) -> Result<AgentSession, CommandError> {
    let session = files.session(id)?;
    if session.id != id || session.parent_session_id.is_some() || session.delegation_depth != 0 {
        return Err(forbidden());
    }
    Ok(session)
}

/// 活动控制器同时绑定窗口与 Vault，过期 runId 不能跨会话发消息。
fn control(
    app: &tauri::AppHandle,
    owner: &str,
    id: &str,
) -> Result<Arc<AgentControl>, CommandError> {
    let control = app.state::<AgentTasks>().get(owner, id)?;
    let current = app.state::<VaultState>().root()?.ok_or_else(forbidden)?;
    if std::path::Path::new(&control.root) != current {
        return Err(forbidden());
    }
    Ok(control)
}

/// 活动树和冷目录都在固定 Vault 阻塞任务内读取，每次查询最多一次目录扫描。
pub(crate) async fn children(
    app: &tauri::AppHandle,
    owner: &str,
    session_id: &str,
    run_id: Option<&str>,
) -> Result<Vec<ChildInfo>, CommandError> {
    let root_path = crate::agent_observation::current_root(app)?;
    let control = run_id
        .map(|id| {
            crate::agent_observation::control(
                &app.state::<AgentTasks>(),
                owner,
                &root_path,
                session_id,
                id,
            )
        })
        .transpose()?
        .flatten();
    let session_id = session_id.to_owned();
    crate::agent_observation::read_at(app, root_path, move |files| {
        crate::agent_observation::authorize_history(files, &session_id, None)?;
        if let Some(tree) = control.as_ref().and_then(|control| control.subagents()) {
            // 专用只读入口遇到关闭仍复用同一份目录，不以二次扫描作回退。
            return tree.observe_children(&session_id);
        }
        stored_children(files, &session_id)
    })
    .await
}

/// 共用服务父索引，不能把每个父节点的子查询变成全库扫描。
fn stored_children(files: &AgentFiles, session_id: &str) -> Result<Vec<ChildInfo>, CommandError> {
    crate::services::subagents::stored_children(files, &root(files, session_id)?)
}

/// 仅查看真实后代的安全历史；读盘与解析不占用同步 Tauri 命令线程。
pub(crate) async fn child_session(
    app: &tauri::AppHandle,
    session_id: &str,
    child_id: &str,
) -> Result<AgentSession, CommandError> {
    let session_id = session_id.to_owned();
    let child_id = child_id.to_owned();
    crate::agent_observation::read(app, move |files| {
        crate::agent_observation::authorize_history(files, &session_id, Some(&child_id))?;
        files.public_session(&child_id)
    })
    .await
}

/// 单轮中断只作用于受当前根拥有的后代，不改变根 Goal 或兄弟任务。
pub(crate) fn interrupt(
    app: &tauri::AppHandle,
    owner: &str,
    run_id: &str,
    child_id: &str,
) -> Result<(), CommandError> {
    let control = control(app, owner, run_id)?;
    let tree = control.subagents().ok_or_else(inactive)?;
    tree.interrupt(&control.snapshot().session_id, child_id)
}

/// UI 明确追加消息仍沿直接父子边传递，空闲根必须先通过人类聊天重新启动。
pub(crate) async fn message(
    app: &tauri::AppHandle,
    owner: &str,
    run_id: &str,
    child_id: &str,
    message: &str,
) -> Result<(), CommandError> {
    let control = control(app, owner, run_id)?;
    let tree = control.subagents().ok_or_else(inactive)?;
    tree.send(&control.snapshot().session_id, child_id, message)
        .await?;
    Ok(())
}

/// 路径和凭据不进入权限错误。
fn forbidden() -> CommandError {
    CommandError::new("SUBAGENT_FORBIDDEN", "子 Agent 不属于当前会话或知识库")
}
/// 驻留实例不存在时要求明确续聊，不能从查询隐式恢复网络执行。
fn inactive() -> CommandError {
    CommandError::new(
        "SUBAGENT_INACTIVE",
        "根任务已结束，请通过主会话追加要求继续",
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{services::subagents::ChildRepository, storage::testutil::TempDir};
    /// 测试通过真实原子存储构造子身份，不跳过 UUID 与父关系检查。
    fn child(files: &AgentFiles, parent: &AgentSession) -> AgentSession {
        let child = AgentSession {
            id: uuid::Uuid::now_v7().to_string(),
            format_version: 1,
            parent_session_id: Some(parent.id.clone()),
            delegation_depth: parent.delegation_depth + 1,
            ..Default::default()
        };
        ChildRepository::create(files, &child).unwrap();
        child
    }
    #[test]
    /// 查看孙会话允许真实父链，但其他根和根自身不能伪装子身份。
    fn lineage_checks_root_and_depth() {
        let temp = TempDir::new();
        let files = AgentFiles::new(temp.path()).unwrap();
        let parent = files.create_session().unwrap();
        let other = files.create_session().unwrap();
        let direct = child(&files, &parent);
        let grandchild = child(&files, &direct);
        assert!(crate::agent_observation::authorize_history(
            &files,
            &parent.id,
            Some(&grandchild.id)
        )
        .is_ok());
        assert!(crate::agent_observation::authorize_history(
            &files,
            &other.id,
            Some(&grandchild.id)
        )
        .is_err());
        assert!(
            crate::agent_observation::authorize_history(&files, &parent.id, Some(&parent.id))
                .is_err()
        );
        assert_eq!(stored_children(&files, &parent.id).unwrap().len(), 2);
        assert!(root(&files, &direct.id).is_err());
    }
}
