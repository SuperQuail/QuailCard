use tauri::{Manager, State};

use crate::{
    dictionary::Dictionary,
    error::CommandError,
    models::{GenerationInput, GenerationTaskStart, GenerationTaskStatus},
    services::AppServices,
    storage::Storage,
};

/// 先登记任务再启动后台执行，确保返回 taskId 后立即停止不会发生竞态。
#[tauri::command]
pub async fn start_generation(
    app: tauri::AppHandle,
    window: tauri::Window,
    services: State<'_, AppServices>,
    input: GenerationInput,
) -> Result<GenerationTaskStart, CommandError> {
    let (task_id, control) = services.generation_tasks.register(window.label())?;
    tauri::async_runtime::spawn(async move {
        let services = app.state::<AppServices>();
        let storage = app.state::<Storage>();
        let dictionary = app.state::<Dictionary>();
        let result = services
            .generate_cards_controlled(&storage, &dictionary, input, &control)
            .await;
        control.complete(result);
    });
    Ok(GenerationTaskStart { task_id })
}

/// 状态查询只向启动窗口返回任务信息。
#[tauri::command]
pub fn get_generation_status(
    window: tauri::Window,
    services: State<'_, AppServices>,
    task_id: String,
) -> Result<GenerationTaskStatus, CommandError> {
    services.generation_tasks.status(window.label(), &task_id)
}

/// 停止幂等且不中途清除已有草稿，由执行器完成终态收尾。
#[tauri::command]
pub fn cancel_generation(
    window: tauri::Window,
    services: State<'_, AppServices>,
    task_id: String,
) -> Result<GenerationTaskStatus, CommandError> {
    services.generation_tasks.cancel(window.label(), &task_id)?;
    services.generation_tasks.status(window.label(), &task_id)
}
