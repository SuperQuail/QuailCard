mod ai;
mod attachment_commands;
mod commands;
mod dictionary;
mod error;
mod models;
mod scheduler;
mod services;
mod storage;
mod vault;
mod vault_crypto;
mod vaultfs;

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
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
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
            commands::save_card,
            commands::delete_card,
            commands::list_note_cards,
            commands::adopt_cards,
            commands::search,
            // 复习
            commands::get_review_queue,
            commands::check_dictation,
            commands::submit_review,
            commands::evaluate_answer,
            // 生成
            commands::generate_cards,
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
