use super::{sanitize_agent_note, sanitize_relative, VaultState};
use crate::error::CommandError;
use std::path::{Component, PathBuf};

impl VaultState {
    /// 检查每个已存在祖先及目标，拒绝链接穿越与 Windows 路径别名。
    pub(crate) fn safe_path(&self, relative: &str) -> Result<PathBuf, CommandError> {
        let relative = sanitize_relative(relative)?;
        let root = self
            .root()?
            .ok_or_else(|| CommandError::new("VAULT_NOT_OPEN", "请先选择知识库"))?;
        let mut target = root.clone();
        for component in relative.components() {
            if let Component::Normal(name) = component {
                let name = name.to_string_lossy();
                let stem = name.split('.').next().unwrap_or("").to_ascii_uppercase();
                if name.ends_with(['.', ' '])
                    || name.contains(':')
                    || [
                        "CON", "PRN", "AUX", "NUL", "COM1", "COM2", "COM3", "COM4", "COM5", "COM6",
                        "COM7", "COM8", "COM9", "LPT1", "LPT2", "LPT3", "LPT4", "LPT5", "LPT6",
                        "LPT7", "LPT8", "LPT9",
                    ]
                    .contains(&stem.as_str())
                {
                    return Err(CommandError::validation("文件路径包含不支持的名称"));
                }
                target.push(name.as_ref());
                if let Ok(metadata) = std::fs::symlink_metadata(&target) {
                    if metadata.file_type().is_symlink()
                        || !target.canonicalize()?.starts_with(&root)
                    {
                        return Err(CommandError::validation("不允许通过链接访问文件"));
                    }
                }
            }
        }
        Ok(target)
    }

    /// Agent 仅访问普通 Markdown，内部目录与隐藏目录不作为材料。
    pub(crate) fn agent_note_path(&self, relative: &str) -> Result<PathBuf, CommandError> {
        self.safe_path(&sanitize_agent_note(relative)?)
    }
}
