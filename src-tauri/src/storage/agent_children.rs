//! 子会话独立存储；父关系是权威，创建不覆盖正在运行的父会话。
use super::*;
use crate::services::subagents::{ChildRecord, ChildRepository, ChildSnapshot};
#[cfg(test)]
#[path = "agent_children_tests.rs"]
mod tests;

impl ChildRepository for AgentFiles {
    /// 创建仅允许全新子身份；主目录无需多文件提交即可由父关系重建。
    fn create(&self, session: &AgentSession) -> Result<(), CommandError> {
        let _guard = session_write_lock()?;
        let parent = session
            .parent_session_id
            .as_deref()
            .ok_or_else(|| CommandError::validation("子会话缺少父身份"))?;
        let parent_session = self.session(parent)?;
        if parent_session.delegation_depth.checked_add(1) != Some(session.delegation_depth) {
            return Err(CommandError::validation("子会话派生深度无效"));
        }
        let path = self.record_path("sessions", &session.id)?;
        if path.exists() {
            return Err(CommandError::validation("子会话身份已存在"));
        }
        self.invalidate_observation(&session.id)?;
        envelope::save_json(&path, session)?;
        self.cache_saved_session(session)
    }

    /// 恢复不执行旧工具，权限与父关系由管理器再次校验。
    fn load(&self, id: &str) -> Result<AgentSession, CommandError> {
        self.session(id)
    }

    /// 单父查询保留旧仓库契约；完整子树消费者应取 snapshot，避免逐父扫描。
    fn list(&self, parent_id: &str) -> Result<Vec<AgentSession>, CommandError> {
        let _ = self.session(parent_id)?;
        let mut children = Vec::new();
        self.visit_child_sessions(|session| {
            if session.parent_session_id.as_deref() == Some(parent_id) {
                children.push(session);
            }
            Ok(())
        })?;
        children.sort_by(|a, b| a.id.cmp(&b.id));
        Ok(children)
    }

    /// 一次会话目录扫描构造父索引；单个会话解析后仅保留轻量目录元数据。
    fn snapshot(&self) -> Result<Option<ChildSnapshot>, CommandError> {
        let mut records = Vec::new();
        self.visit_child_sessions(|session| {
            records.push(ChildRecord::from_session(&session));
            Ok(())
        })?;
        Ok(Some(ChildSnapshot::from_records(records)?))
    }
}

impl AgentFiles {
    /// 枚举路径仍经沙箱净化和 UUID 校验，文件名与记录身份不一致时拒绝目录。
    fn visit_child_sessions(
        &self,
        mut visit: impl FnMut(AgentSession) -> Result<(), CommandError>,
    ) -> Result<(), CommandError> {
        let directory = self.vault.safe_path(".quailcard/agent/.sessions")?;
        #[cfg(test)]
        CHILD_DIRECTORY_SCANS.with(|count| count.set(count.get() + 1));
        for entry in std::fs::read_dir(directory)? {
            let path = entry?.path();
            if !path.extension().is_some_and(|ext| ext == "json") {
                continue;
            }
            let id = path
                .file_stem()
                .and_then(|name| name.to_str())
                .unwrap_or("");
            let session = self.session(id)?;
            if session.id != id {
                return Err(CommandError::new("SUBAGENT_FORBIDDEN", "子会话身份无效"));
            }
            visit(session)?;
        }
        Ok(())
    }
}

#[cfg(test)]
thread_local! {
    static CHILD_DIRECTORY_SCANS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

#[cfg(test)]
#[path = "agent_child_snapshot_tests.rs"]
mod snapshot_tests;
