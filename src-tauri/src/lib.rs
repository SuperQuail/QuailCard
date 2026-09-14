mod agent_autonomy_bridge;
mod agent_autonomy_commands;
mod agent_autonomy_models;
mod agent_bridge;
mod agent_cards;
mod agent_child_executor;
mod agent_commands;
mod agent_execution_settings;
mod agent_learning;
mod agent_models;
mod agent_observation;
mod agent_public_history;
mod agent_video;
mod agent_writes;
mod ai;
#[cfg(test)]
mod architecture_tests;
mod attachment_commands;
mod card_commands;
mod card_generation;
mod commands;
mod dictionary;
mod error;
mod generation_commands;
mod models;
mod paths;
mod scheduler;
mod services;
mod storage;
mod vault;
mod vault_crypto;
mod vaultfs;
mod video;
mod video_bridge;
mod video_commands;
mod video_history;
mod video_login_commands;

use std::path::PathBuf;

use tauri::{Manager, Runtime};
use vaultfs::VaultState;

/// 定位内置 ECDICT 词典文件：发布时读取打包资源，开发时回退到源码资源目录。
fn resolve_dictionary_path<R: Runtime>(
    app: &tauri::App<R>,
) -> Result<PathBuf, Box<dyn std::error::Error>> {
    let resource_path = app.path().resource_dir()?.join("ecdict.db");
    if resource_path.exists() {
        return Ok(resource_path);
    }
    let dev_path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("resources/ecdict.db");
    Ok(dev_path)
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
/// 创建并运行 QuailCard 的 Tauri 应用实例。
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .setup(|app| {
            let app_data_dir = app.path().app_data_dir()?;
            std::fs::create_dir_all(&app_data_dir)?;
            let storage = storage::Storage::open(&app_data_dir)?;
            let services = services::AppServices::new()?;
            tauri::async_runtime::block_on(services.initialize(&storage))?;
            let dictionary = dictionary::Dictionary::connect(&resolve_dictionary_path(app)?)?;
            let speech = services::SpeechService::new(app_data_dir.join("speech"));
            app.manage(storage);
            app.manage(services);
            app.manage(dictionary);
            app.manage(speech);
            app.manage(VaultState::new());
            app.manage(services::agent_tasks::AgentTasks::default());
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            agent_autonomy_commands::agent_children,
            agent_autonomy_commands::agent_child_session,
            agent_autonomy_commands::agent_interrupt_child,
            agent_autonomy_commands::agent_message_child,
            agent_commands::agent_observe_session,
            agent_commands::agent_pending_writes,
            agent_commands::agent_sessions,
            agent_commands::agent_create_session,
            agent_commands::agent_session,
            agent_commands::agent_delete_session,
            agent_commands::agent_send,
            agent_commands::agent_status,
            agent_commands::agent_cancel,
            agent_commands::agent_acknowledge_write,
            agent_commands::agent_change,
            agent_commands::agent_undo,
            agent_commands::agent_memory,
            agent_commands::agent_save_memory,
            agent_commands::agent_save_review,
            agent_commands::agent_adopt_cards,
            // Vault 图片附件
            attachment_commands::get_vault_config,
            attachment_commands::set_attachment_folder,
            attachment_commands::import_note_attachment,
            attachment_commands::read_note_attachment,
            // Vault 与笔记
            commands::open_vault,
            commands::get_vault_path,
            commands::get_recent_vaults,
            commands::sync_note_index,
            commands::rescan_vault,
            commands::list_notes,
            commands::read_note,
            commands::write_note,
            commands::create_note_file,
            commands::create_folder,
            commands::rename_note_file,
            commands::delete_note_file,
            commands::rename_folder,
            commands::delete_folder,
            // 卡片与搜索
            card_commands::save_card,
            card_commands::delete_card,
            card_commands::list_note_cards,
            card_commands::adopt_cards,
            card_commands::search,
            // 复习
            card_commands::get_review_queue,
            card_commands::check_dictation,
            card_commands::submit_review,
            card_commands::evaluate_answer,
            // 生成
            card_commands::generate_cards,
            generation_commands::start_generation,
            generation_commands::get_generation_status,
            generation_commands::cancel_generation,
            // 视频转笔记
            video_commands::video_probe,
            video_login_commands::video_login_start,
            video_login_commands::video_login_status,
            video_login_commands::video_login_cancel,
            video_login_commands::video_avatar,
            video_login_commands::video_logout,
            video_commands::video_task_start,
            video_commands::video_task_status,
            video_commands::video_task_cancel,
            video_commands::video_components,
            video_commands::video_models,
            video_commands::video_download_model,
            video_commands::video_model_cancel,
            video_commands::video_download_status,
            video_commands::video_history,
            video_commands::video_get_settings,
            video_commands::video_save_settings,
            // 供应商、保险库与系统
            commands::get_bootstrap_data,
            commands::set_font_size,
            commands::get_study_stats,
            commands::set_ai_grading_enabled,
            commands::set_active_provider,
            commands::save_provider,
            commands::delete_provider,
            commands::test_provider,
            commands::start_openai_login,
            commands::get_openai_login_status,
            commands::cancel_openai_login,
            commands::logout_openai,
            commands::get_vault_status,
            commands::unlock_vault,
            commands::set_vault_password,
            commands::remove_vault_password,
            commands::lock_vault,
            commands::reset_vault,
            commands::lookup_dictionary_word,
            commands::synthesize_speech,
            commands::get_data_locations,
            commands::reveal_data_folder,
        ])
        .run(tauri::generate_context!())
        .expect("QuailCard 启动失败");
}
