use tauri::State;
use tauri_plugin_dialog::DialogExt;

use crate::{
    error::CommandError,
    image_clipboard,
    models::{
        AttachmentImage, ImportNoteAttachmentInput, ImportedAttachment, ReadNoteAttachmentInput,
        VaultConfig,
    },
    vaultfs::{attachment_file_hints, read_attachment_bytes, write_replacing, VaultState},
};

/// 返回当前 Vault 的独立附件配置。
#[tauri::command]
pub fn get_vault_config(vault: State<'_, VaultState>) -> Result<VaultConfig, CommandError> {
    vault.get_config()
}

/// 校验并持久化当前 Vault 的附件目录。
#[tauri::command]
pub fn set_attachment_folder(
    vault: State<'_, VaultState>,
    attachment_folder: String,
) -> Result<VaultConfig, CommandError> {
    vault.set_attachment_folder(&attachment_folder)
}

/// 将图片安全导入当前 Vault 并返回笔记相对路径。
#[tauri::command]
pub fn import_note_attachment(
    vault: State<'_, VaultState>,
    input: ImportNoteAttachmentInput,
) -> Result<ImportedAttachment, CommandError> {
    vault.import_note_attachment(input)
}

/// 读取并验证笔记引用的 Vault 内图片。
#[tauri::command]
pub fn read_note_attachment(
    vault: State<'_, VaultState>,
    input: ReadNoteAttachmentInput,
) -> Result<AttachmentImage, CommandError> {
    vault.read_note_attachment(input)
}

/// 把笔记内图片解码为位图后写入系统剪贴板。
///
/// 解码与剪贴板写入都可能阻塞（解码 10 MiB 图片、剪贴板被占用时重试），
/// 因此放到阻塞线程池，不占用异步运行时的工作线程。
#[tauri::command]
pub async fn copy_note_attachment_image(
    vault: State<'_, VaultState>,
    note_path: String,
    source: String,
) -> Result<(), CommandError> {
    let bytes = vault.read_attachment(&note_path, &source)?;
    tauri::async_runtime::spawn_blocking(move || {
        let image = image_clipboard::decode_rgba(&bytes)?;
        image_clipboard::copy_to_clipboard(image)
    })
    .await
    .map_err(|error| {
        eprintln!("CLIPBOARD_TASK_ERROR(detail): {error}");
        CommandError::new("CLIPBOARD_ERROR", "无法写入系统剪贴板，请稍后重试")
    })?
}

/// 另存笔记内图片到用户选择的位置；用户取消时返回 None。
///
/// 保存对话框只能在非主线程阻塞调用，因此和写盘一起放进阻塞线程池；
/// 前端不提交任何路径，目标位置完全由用户在本机对话框中选择。
#[tauri::command]
pub async fn save_note_attachment_as(
    app: tauri::AppHandle,
    vault: State<'_, VaultState>,
    note_path: String,
    source: String,
) -> Result<Option<String>, CommandError> {
    let path = vault.attachment_file_path(&note_path, &source)?;
    let bytes = read_attachment_bytes(&path)?;
    let (file_name, extension) = attachment_file_hints(&path, &bytes)?;
    let app_handle = app.clone();
    let chosen = tauri::async_runtime::spawn_blocking(move || {
        app_handle
            .dialog()
            .file()
            .set_file_name(file_name)
            .add_filter("图片", &[extension])
            .blocking_save_file()
    })
    .await
    .map_err(|error| {
        eprintln!("SAVE_ATTACHMENT_TASK_ERROR(detail): {error}");
        CommandError::new("SAVE_ERROR", "保存图片失败")
    })?;
    let Some(file_path) = chosen else {
        return Ok(None);
    };
    let target = file_path.into_path().map_err(|error| {
        eprintln!("SAVE_ATTACHMENT_PATH_ERROR(detail): {error}");
        CommandError::validation("另存位置无效")
    })?;
    write_replacing(&target, &bytes)?;
    Ok(Some(target.to_string_lossy().into_owned()))
}

/// 在系统文件管理器中定位并选中笔记引用的图片。
///
/// 路径由后端从笔记与图片引用解析，前端提交的永远只是相对引用。
#[tauri::command]
pub fn reveal_note_attachment(
    app: tauri::AppHandle,
    vault: State<'_, VaultState>,
    note_path: String,
    source: String,
) -> Result<(), CommandError> {
    use tauri_plugin_opener::OpenerExt;
    let path = vault.attachment_file_path(&note_path, &source)?;
    app.opener().reveal_item_in_dir(&path).map_err(|error| {
        eprintln!("OPEN_FOLDER_ERROR(detail): {error}");
        CommandError::new("OPEN_FOLDER_ERROR", "无法打开文件所在文件夹")
    })
}
