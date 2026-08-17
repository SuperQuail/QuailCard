use tauri::State;

use crate::{
    error::CommandError,
    models::{
        AttachmentImage, ImportNoteAttachmentInput, ImportedAttachment, ReadNoteAttachmentInput,
        VaultConfig,
    },
    vaultfs::VaultState,
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
