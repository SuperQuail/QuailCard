//! 视频路径边界与旧文件只读兼容；不执行迁移、备份或删除。
use std::path::{Path, PathBuf};

use serde::de::DeserializeOwned;

use crate::{error::CommandError, vaultfs::VaultState};

pub(super) const CURRENT: &str = ".quailcard/agent/.video";
pub(super) const LEGACY: &str = ".quailcard/video";

/// 调用方输入仅允许无歧义单组件，Windows 别名继续交由 vaultfs 拒绝。
pub(super) fn component(value: &str) -> Result<(), CommandError> {
    if value.is_empty()
        || value == "."
        || value == ".."
        || value.trim() != value
        || !value
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'))
    {
        return Err(CommandError::validation("视频文件标识不合法"));
    }
    crate::vaultfs::sanitize_relative(value)?;
    Ok(())
}

/// 根目录必须已经存在；所有祖先与最终目标均由 vaultfs 校验。
pub(super) fn resolve(
    root: &Path,
    namespace: &str,
    relative: &str,
) -> Result<PathBuf, CommandError> {
    let vault = VaultState::new();
    vault.set_root(root.to_path_buf())?;
    vault.safe_path(&format!("{namespace}/{relative}"))
}

/// 仅无损旧键可兼容；复合键绝不能退化到旧视频号缓存。
pub(super) fn legacy_key(value: &str) -> bool {
    !value.is_empty() && value.chars().all(|c| c.is_ascii_alphanumeric() || c == '-')
}

/// 旧数据与卡片共用目录，先排除卡片和源笔记，再只读解析视频形状。
pub(super) fn legacy_json<T: DeserializeOwned>(
    root: &Path,
    relative: &str,
    marker: &str,
) -> Result<Option<T>, CommandError> {
    let path = resolve(root, LEGACY, relative)?;
    let note = format!("video/{}.md", relative.trim_end_matches(".json"));
    let vault = VaultState::new();
    vault.set_root(root.to_path_buf())?;
    if vault.safe_path(&note)?.try_exists()? {
        return Ok(None);
    }
    let bytes = match std::fs::read(&path) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error.into()),
    };
    let Ok(value) = serde_json::from_slice::<serde_json::Value>(&bytes) else {
        return Ok(None);
    };
    if value.get("cards").is_some() || value.get(marker).is_none() {
        return Ok(None);
    }
    if value
        .get("formatVersion")
        .and_then(|v| v.as_u64())
        .unwrap_or(0)
        > super::FORMAT_VERSION
    {
        return Err(CommandError::new(
            "FILE_FORMAT_NEWER",
            "视频文件版本高于当前应用",
        ));
    }
    Ok(serde_json::from_value(value).ok())
}
