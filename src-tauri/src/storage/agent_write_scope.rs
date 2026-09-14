//! 写入授权范围的存在性校验：父级只能授予真实存在的笔记或目录。
//!
//! 纯字符串净化与分段匹配在 services::agent_write_scope；这里只补 vaultfs 沙箱校验，
//! 让"根可授权任意存在的路径"有唯一出口。

use super::AgentFiles;
use crate::{
    error::CommandError,
    services::agent_write_scope::{self, ScopeEntry},
};

impl AgentFiles {
    /// 校验并规范化父级提交的写入范围；目录前缀校验目录存在，精确路径校验笔记存在。
    ///
    /// 子代理之间的再授权不走这里，由管理器的父范围子集校验负责（子范围可能在父目录内新建文件）。
    pub(crate) fn validate_write_scope(
        &self,
        scope: &[String],
    ) -> Result<Vec<String>, CommandError> {
        let entries = agent_write_scope::parse(scope)?;
        for entry in &entries {
            self.ensure_scope_target(entry)?;
        }
        Ok(agent_write_scope::canonical(&entries))
    }

    /// 存在性判断复用 vaultfs 沙箱路径，错误不回显原始路径。
    fn ensure_scope_target(&self, entry: &ScopeEntry) -> Result<(), CommandError> {
        match entry {
            // 根目录前缀：VaultState::set_root 已确认知识库根存在。
            ScopeEntry::Dir(prefix) if prefix.is_empty() => Ok(()),
            ScopeEntry::Dir(prefix) => {
                if self.vault.safe_path(prefix)?.is_dir() {
                    Ok(())
                } else {
                    Err(missing())
                }
            }
            ScopeEntry::File(note) => {
                if self.vault.agent_note_path(note)?.is_file() {
                    Ok(())
                } else {
                    Err(missing())
                }
            }
        }
    }
}

/// 稳定错误码 + 安全中文消息：不暴露磁盘路径与仓库内部细节。
fn missing() -> CommandError {
    CommandError::new(
        "AGENT_WRITE_SCOPE_DENIED",
        "写入授权路径不存在：目录前缀必须指向已存在的文件夹，精确路径必须指向已存在的 Markdown 笔记",
    )
}
