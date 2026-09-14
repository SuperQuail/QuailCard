//! 自主任务界面命令仅接收参数、注入窗口身份并委派。
use crate::{
    agent_autonomy_bridge as bridge, agent_models::AgentSession, error::CommandError,
    services::subagents::ChildInfo,
};

/// 刷新只读取目录或活动状态，不创建模型任务。
#[tauri::command]
pub async fn agent_children(
    app: tauri::AppHandle,
    window: tauri::Window,
    session_id: String,
    run_id: Option<String>,
) -> Result<Vec<ChildInfo>, CommandError> {
    bridge::children(&app, window.label(), &session_id, run_id.as_deref()).await
}

/// 查看只返回已去除供应商重放块的授权子历史。
#[tauri::command]
pub async fn agent_child_session(
    app: tauri::AppHandle,
    session_id: String,
    child_id: String,
) -> Result<AgentSession, CommandError> {
    bridge::child_session(&app, &session_id, &child_id).await
}

/// 中断当前后代轮次，不把操作转成停止根目标。
#[tauri::command]
pub fn agent_interrupt_child(
    app: tauri::AppHandle,
    window: tauri::Window,
    run_id: String,
    child_id: String,
) -> Result<(), CommandError> {
    bridge::interrupt(&app, window.label(), &run_id, &child_id)
}

/// 用户追加指令仍受活动根、父关系、范围与邮箱额度检查。
#[tauri::command]
pub async fn agent_message_child(
    app: tauri::AppHandle,
    window: tauri::Window,
    run_id: String,
    child_id: String,
    message: String,
) -> Result<(), CommandError> {
    bridge::message(&app, window.label(), &run_id, &child_id, &message).await
}
