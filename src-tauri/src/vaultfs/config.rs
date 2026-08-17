use std::path::Path;

use uuid::Uuid;

use super::{
    attachments::{ensure_contained, file_error, write_new_file},
    VaultState,
};
use crate::{error::CommandError, models::VaultConfig};

const DEFAULT_ATTACHMENT_FOLDER: &str = "attachments";

impl Default for VaultConfig {
    /// 缺少配置文件时使用稳定且可迁移的默认附件目录。
    fn default() -> Self {
        Self {
            attachment_folder: DEFAULT_ATTACHMENT_FOLDER.to_string(),
        }
    }
}

impl VaultState {
    /// 读取当前 Vault 配置；首次读取会写入独立默认配置。
    pub fn get_config(&self) -> Result<VaultConfig, CommandError> {
        let root = self.require_root()?;
        let path = root.join(".quailcard/config.json");
        if !path.exists() {
            let config = VaultConfig::default();
            persist_config(&root, &config)?;
            return Ok(config);
        }
        let canonical = path
            .canonicalize()
            .map_err(|error| file_error("读取 Vault 配置失败", error))?;
        ensure_contained(&root, &canonical)?;
        if !canonical.is_file() {
            return Err(CommandError::new(
                "VAULT_CONFIG_INVALID",
                "Vault 配置文件无效",
            ));
        }
        let bytes =
            std::fs::read(canonical).map_err(|error| file_error("读取 Vault 配置失败", error))?;
        let mut config: VaultConfig = serde_json::from_slice(&bytes).map_err(|error| {
            eprintln!("VAULT_CONFIG_ERROR(detail): {error}");
            CommandError::new("VAULT_CONFIG_INVALID", "Vault 配置文件无效")
        })?;
        config.attachment_folder = normalize_attachment_folder(&config.attachment_folder)?;
        Ok(config)
    }

    /// 校验附件目录并以可恢复替换方式持久化配置。
    pub fn set_attachment_folder(&self, folder: &str) -> Result<VaultConfig, CommandError> {
        let root = self.require_root()?;
        let config = VaultConfig {
            attachment_folder: normalize_attachment_folder(folder)?,
        };
        persist_config(&root, &config)?;
        Ok(config)
    }
}

/// 附件目录严格采用 `/` 分隔的 Vault 相对普通路径。
pub(super) fn normalize_attachment_folder(folder: &str) -> Result<String, CommandError> {
    let folder = folder.trim();
    if folder.is_empty() || folder.contains('\\') || folder.starts_with('/') {
        return Err(CommandError::validation("附件目录必须是 Vault 相对路径"));
    }
    let mut normalized = Vec::new();
    for segment in folder.split('/') {
        if segment.is_empty()
            || matches!(segment, "." | "..")
            || segment.eq_ignore_ascii_case(".quailcard")
            || segment.contains(':')
            || segment.chars().any(char::is_control)
        {
            return Err(CommandError::validation("附件目录包含非法路径段"));
        }
        normalized.push(segment);
    }
    Ok(normalized.join("/"))
}

/// 将 Vault 配置写入同目录临时文件，再以可恢复替换更新正式文件。
fn persist_config(root: &Path, config: &VaultConfig) -> Result<(), CommandError> {
    let metadata = root.join(".quailcard");
    std::fs::create_dir_all(&metadata).map_err(|error| file_error("保存 Vault 配置失败", error))?;
    let metadata = metadata
        .canonicalize()
        .map_err(|error| file_error("保存 Vault 配置失败", error))?;
    ensure_contained(root, &metadata)?;
    let target = metadata.join("config.json");
    let temporary = metadata.join(format!("config.{}.tmp", Uuid::now_v7()));
    let backup = metadata.join(format!("config.{}.bak", Uuid::now_v7()));
    let bytes = serde_json::to_vec_pretty(config).map_err(|error| {
        eprintln!("VAULT_CONFIG_ERROR(detail): {error}");
        CommandError::new("INTERNAL_ERROR", "保存 Vault 配置失败")
    })?;
    write_new_file(&temporary, &bytes, "保存 Vault 配置失败")?;
    if target.exists() {
        if let Err(error) = std::fs::rename(&target, &backup) {
            let _ = std::fs::remove_file(&temporary);
            return Err(file_error("保存 Vault 配置失败", error));
        }
    }
    if let Err(error) = std::fs::rename(&temporary, &target) {
        if backup.exists() {
            let _ = std::fs::rename(&backup, &target);
        }
        let _ = std::fs::remove_file(&temporary);
        return Err(file_error("保存 Vault 配置失败", error));
    }
    if backup.exists() {
        if let Err(error) = std::fs::remove_file(&backup) {
            eprintln!("VAULT_CONFIG_BACKUP_CLEANUP_ERROR(detail): {error}");
        }
    }
    Ok(())
}
