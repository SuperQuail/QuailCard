use super::{AgentFiles, CORRUPT};
use crate::storage::envelope;
use crate::{agent_models::AgentChange, error::CommandError};
use sha2::{Digest, Sha256};

/// 哈希比较真实 UTF-8 字节，不能用秒级修改时间代替版本。
pub(super) fn hash(content: &str) -> String {
    format!("{:x}", Sha256::digest(content.as_bytes()))
}

impl AgentFiles {
    /// 先写恢复日志再原子写笔记，重复调用只返回已确认的操作。
    pub(super) fn apply_change(
        &self,
        id: &str,
        path: &str,
        content: &str,
        expected: Option<&str>,
    ) -> Result<AgentChange, CommandError> {
        if content.len() > 256 * 1024 {
            return Err(CommandError::validation("单次笔记内容不能超过 256 KiB"));
        }
        // 锁键取净化后的规范路径：别名写法不能落到两把锁上。
        // 顺序契约：这里只取路径锁，绝不再取 SESSION_WRITE（见 agent_path_locks）。
        let key = crate::vaultfs::sanitize_agent_note(path)?;
        let lock = super::path_locks::lock(&key)?;
        let _guard = super::path_locks::enter(&lock);
        let target = self.vault.agent_note_path(&key)?;
        let record = self.record_path("changes", id)?;
        if let Some(change) = envelope::load_json::<AgentChange>(&record, &CORRUPT)? {
            return self.reconcile(change);
        }
        let before = match std::fs::read_to_string(&target) {
            Ok(content) => Some(content),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
            Err(error) => return Err(error.into()),
        };
        if before.as_deref().map(hash).as_deref() != expected {
            return Err(CommandError::new(
                "AGENT_NOTE_CONFLICT",
                "笔记已变化或同名文件已存在，请重新读取后再修改",
            ));
        }
        let mut change = AgentChange {
            format_version: 1,
            id: id.into(),
            path: key,
            before_hash: before.as_deref().map(hash),
            after_hash: hash(content),
            before,
            after: content.into(),
            state: "pending".into(),
        };
        envelope::save_json(&record, &change)?;
        // 日志保存后再检查一次，缩小外部编辑竞争窗口。
        let current = std::fs::read_to_string(&target).ok();
        if current.as_deref().map(hash) != change.before_hash {
            return Err(CommandError::new(
                "AGENT_NOTE_CONFLICT",
                "笔记在写入前发生变化，已停止修改",
            ));
        }
        envelope::write_atomic(&target, content.as_bytes())?;
        change.state = "applied".into();
        envelope::save_json(&record, &change)?;
        Ok(change)
    }

    /// 崩溃恢复只核对文件，不自动重放待执行写入。
    fn reconcile(&self, mut change: AgentChange) -> Result<AgentChange, CommandError> {
        if change.state == "pending" || change.state == "undoing" {
            let target = self.vault.agent_note_path(&change.path)?;
            let current = std::fs::read_to_string(target).ok().as_deref().map(hash);
            change.state = if current.as_deref() == Some(&change.after_hash) {
                "applied"
            } else if current == change.before_hash {
                if change.state == "undoing" {
                    "undone"
                } else {
                    "notApplied"
                }
            } else {
                "conflict"
            }
            .into();
            envelope::save_json(&self.record_path("changes", &change.id)?, &change)?;
        }
        Ok(change)
    }

    /// 获取差异时同时核对未完成日志，永远展示真实文件状态。
    pub(crate) fn get_change(&self, id: &str) -> Result<AgentChange, CommandError> {
        let change = envelope::load_json(&self.record_path("changes", id)?, &CORRUPT)?
            .ok_or_else(|| CommandError::new("AGENT_CHANGE_MISSING", "改动记录不存在"))?;
        self.reconcile(change)
    }

    /// 撤销仅恢复没有后续编辑的文件，新建笔记以回收副本保留。
    pub(crate) fn undo(&self, id: &str, has_cards: bool) -> Result<AgentChange, CommandError> {
        let mut change = self.get_change(id)?;
        if change.state == "undone" {
            return Ok(change);
        }
        // 撤销同样是笔记写入：与 apply_change 共用同一把按路径锁。
        let key = crate::vaultfs::sanitize_agent_note(&change.path)?;
        let lock = super::path_locks::lock(&key)?;
        let _guard = super::path_locks::enter(&lock);
        let target = self.vault.agent_note_path(&key)?;
        let current = std::fs::read_to_string(&target)?;
        if change.state != "applied"
            || hash(&current) != change.after_hash
            || (change.before.is_none() && has_cards)
        {
            return Err(CommandError::new(
                "AGENT_UNDO_CONFLICT",
                "笔记已有后续编辑或关联卡片，无法直接撤销；可查看原始内容",
            ));
        }
        change.state = "undoing".into();
        let record = self.record_path("changes", id)?;
        envelope::save_json(&record, &change)?;
        if let Some(before) = &change.before {
            envelope::write_atomic(&target, before.as_bytes())?;
        } else {
            let recycle = self
                .vault
                .safe_path(&format!(".quailcard/agent/.recycle/{id}.md"))?;
            if let Some(parent) = recycle.parent() {
                std::fs::create_dir_all(parent)?;
            }
            std::fs::rename(target, recycle)?;
        }
        change.state = "undone".into();
        envelope::save_json(&record, &change)?;
        Ok(change)
    }
}
