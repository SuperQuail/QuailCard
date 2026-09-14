use crate::{
    agent_bridge,
    error::CommandError,
    models::{AgentChange, AgentInput, AgentMemory, AgentRun, AgentSession},
    services::{agent_ports::AgentRepository, agent_tasks::AgentTasks},
};
use tauri::{Manager, Window};

/// 只委派会话列表读取，不包含文件处理规则。
#[tauri::command]
pub async fn agent_sessions(app: tauri::AppHandle) -> Result<Vec<AgentSession>, CommandError> {
    crate::agent_observation::read(&app, |files| files.sessions()).await
}
/// 创建会话后返回稳定身份。
#[tauri::command]
pub fn agent_create_session(app: tauri::AppHandle) -> Result<AgentSession, CommandError> {
    agent_bridge::files(&app)?.create_session()
}
/// 读取单个历史，不恢复执行旧工具。
#[tauri::command]
pub async fn agent_session(
    app: tauri::AppHandle,
    id: String,
) -> Result<AgentSession, CommandError> {
    crate::agent_observation::read(&app, move |files| files.public_session(&id)).await
}

/// 版本化观察仅采样当前运行与安全历史；null 历史表示未变，绝不隐式启动模型。
#[tauri::command]
pub async fn agent_observe_session(
    app: tauri::AppHandle,
    window: Window,
    session_id: String,
    run_id: Option<String>,
    child_id: Option<String>,
    known_revision: Option<String>,
) -> Result<crate::agent_observation::AgentObservation, CommandError> {
    crate::agent_observation::observe(
        &app,
        window.label(),
        session_id,
        run_id,
        child_id,
        known_revision,
    )
    .await
}
/// 只读整树待保存写入；历史观察失败时前端仍能完成编辑器保存协调。
#[tauri::command]
pub async fn agent_pending_writes(
    app: tauri::AppHandle,
    window: Window,
    id: String,
) -> Result<Vec<crate::agent_writes::AgentPendingWrite>, CommandError> {
    let control = crate::agent_writes::root(&app, window.label(), &id)?;
    Ok(crate::agent_writes::pending(&control))
}
/// 删除委派给组合层执行，命令不处理会话文件。
#[tauri::command]
pub fn agent_delete_session(app: tauri::AppHandle, id: String) -> Result<(), CommandError> {
    agent_bridge::delete_session(&app, &id)
}
/// 启动委派给应用组合根，命令不持有 HTTP 或业务逻辑。
#[tauri::command]
pub async fn agent_send(
    app: tauri::AppHandle,
    window: Window,
    input: AgentInput,
) -> Result<AgentRun, CommandError> {
    agent_bridge::start(app, window.label().into(), input).await
}
/// 完整任务快照仅对发起窗口开放。
#[tauri::command]
pub fn agent_status(
    app: tauri::AppHandle,
    window: Window,
    id: String,
) -> Result<AgentRun, CommandError> {
    Ok(app
        .state::<AgentTasks>()
        .get(window.label(), &id)?
        .snapshot())
}
/// 停止只取消未完成任务，保留已写入笔记。
#[tauri::command]
pub fn agent_cancel(app: tauri::AppHandle, window: Window, id: String) -> Result<(), CommandError> {
    app.state::<AgentTasks>().get(window.label(), &id)?.cancel();
    Ok(())
}
/// 编辑器完成草稿保存后确认整树中的指定执行；不接受前端提供的写入内容。
#[tauri::command]
pub async fn agent_acknowledge_write(
    app: tauri::AppHandle,
    window: Window,
    id: String,
    execution_id: String,
    operation_id: String,
) -> Result<(), CommandError> {
    let control = crate::agent_writes::root(&app, window.label(), &id)?;
    crate::agent_writes::acknowledge(&control, &execution_id, &operation_id).await
}
/// 采纳由用户点击触发，不能由模型直接写入卡片库。
#[tauri::command]
pub async fn agent_adopt_cards(
    app: tauri::AppHandle,
    session_id: String,
    message_id: String,
    draft_ids: Vec<String>,
) -> Result<AgentSession, CommandError> {
    agent_bridge::adopt(&app, &session_id, &message_id, &draft_ids).await
}

/// 差异按稳定操作 ID 加载。
#[tauri::command]
pub fn agent_change(app: tauri::AppHandle, id: String) -> Result<AgentChange, CommandError> {
    agent_bridge::files(&app)?.get_change(&id)
}
/// 撤销只接受操作身份，不能提交任意恢复正文。
#[tauri::command]
pub async fn agent_undo(app: tauri::AppHandle, id: String) -> Result<AgentChange, CommandError> {
    agent_bridge::undo(&app, &id).await
}
/// 读取显式长期记忆。
#[tauri::command]
pub fn agent_memory(app: tauri::AppHandle) -> Result<AgentMemory, CommandError> {
    agent_bridge::files(&app)?.memory()
}
/// UI 显式保存或清空长期记忆。
#[tauri::command]
pub fn agent_save_memory(
    app: tauri::AppHandle,
    content: String,
) -> Result<AgentMemory, CommandError> {
    agent_bridge::files(&app)?.save_memory(&content)
}

/// 复习快照只影响界面恢复，不允许改变调度和复习历史。
#[tauri::command]
pub fn agent_save_review(
    app: tauri::AppHandle,
    session_id: String,
    message_id: String,
    progress: serde_json::Value,
) -> Result<(), CommandError> {
    app.state::<AgentTasks>().ensure_idle()?;
    agent_bridge::files(&app)?.save_review(&session_id, &message_id, progress)
}
