use super::{
    envelope::{self, CorruptPolicy},
    now_timestamp,
};
use crate::{
    agent_models::{AgentChange, AgentMemory, AgentSession},
    error::CommandError,
    services::agent_ports::AgentRepository,
    vaultfs::VaultState,
};
use serde_json::{json, Value};
use std::path::Path;
use uuid::Uuid;

// 会话删除与保存串行化，避免迟到保存重新创建已回收的记录。
static SESSION_WRITE: std::sync::Mutex<()> = std::sync::Mutex::new(());

/// 锁中毒返回安全错误，禁止在状态不确定时写入历史。
fn session_write_lock() -> Result<std::sync::MutexGuard<'static, ()>, CommandError> {
    SESSION_WRITE
        .lock()
        .map_err(|_| CommandError::new("INTERNAL_ERROR", "会话记录暂时不可用"))
}

#[path = "agent_changes.rs"]
mod changes;
#[path = "agent_children.rs"]
mod children;
#[path = "agent_observation.rs"]
mod observation;
pub(crate) use observation::SessionIdentity;
#[path = "agent_path_locks.rs"]
mod path_locks;
#[path = "agent_write_scope.rs"]
mod write_scope;

/// 测试与并发契约验证使用同一把按路径锁，不另造第二套串行化。
#[cfg(test)]
pub(crate) use path_locks::lock as lock_note_path;
#[cfg(test)]
#[path = "agent_tests.rs"]
mod tests;

const CORRUPT: CorruptPolicy = CorruptPolicy::Reject {
    code: "AGENT_FILE_CORRUPT",
    message: "Agent 记录损坏，请恢复备份后重试；原文件已保留",
};

/// 每个实例绑定固定根目录，切库不会改变后台任务的目标。
pub(crate) struct AgentFiles {
    vault: VaultState,
}

impl AgentFiles {
    /// 在组合层捕获根目录，仅允许访问当前知识库。
    pub(crate) fn new(root: &Path) -> Result<Self, CommandError> {
        let vault = VaultState::new();
        vault.set_root(root.to_path_buf())?;
        Ok(Self { vault })
    }

    /// 身份只能由 UUID 组成，防止会话读取接口形成任意文件访问。
    fn record_path(&self, domain: &str, id: &str) -> Result<std::path::PathBuf, CommandError> {
        Uuid::parse_str(id).map_err(|_| CommandError::validation("Agent 记录 ID 无效"))?;
        self.vault
            .safe_path(&format!(".quailcard/agent/.{domain}/{id}.json"))
    }

    /// 列表只返回摘要，历史正文按会话单独加载。
    pub(crate) fn sessions(&self) -> Result<Vec<AgentSession>, CommandError> {
        let path = self.vault.safe_path(".quailcard/agent/.sessions")?;
        if !path.exists() {
            return Ok(vec![]);
        }
        let mut sessions = Vec::new();
        for entry in std::fs::read_dir(path)? {
            let path = entry?.path();
            if path.extension().is_some_and(|ext| ext == "json") {
                let id = path
                    .file_stem()
                    .and_then(|name| name.to_str())
                    .unwrap_or("");
                let mut session = self.session(id)?;
                if session.parent_session_id.is_none() {
                    session.messages.clear();
                    sessions.push(session);
                }
            }
        }
        sessions.sort_by_key(|session| std::cmp::Reverse(session.updated_at));
        Ok(sessions)
    }

    /// 打开历史不会重新执行工具，未完成轮次以中断消息展示。
    pub(crate) fn session(&self, id: &str) -> Result<AgentSession, CommandError> {
        #[cfg(test)]
        observation::record_read();
        let mut session: AgentSession =
            envelope::load_json(&self.record_path("sessions", id)?, &CORRUPT)?
                .ok_or_else(|| CommandError::new("AGENT_SESSION_MISSING", "会话不存在"))?;
        if session.id != id {
            return Err(CommandError::new(
                "AGENT_FILE_CORRUPT",
                "会话身份与文件不一致",
            ));
        }
        for message in &mut session.messages {
            if message.kind == "running" {
                message.kind = "interrupted".into();
                message.content = "上次任务已中断，已完成操作保留；可发送消息继续。".into();
            }
        }
        Ok(session)
    }

    /// 新会话先落盘，避免首次发送失败后丢失会话身份。
    pub(crate) fn create_session(&self) -> Result<AgentSession, CommandError> {
        let _guard = session_write_lock()?;
        let session = AgentSession {
            format_version: 1,
            id: Uuid::now_v7().to_string(),
            title: "新会话".into(),
            updated_at: now_timestamp(),
            ..Default::default()
        };
        self.invalidate_observation(&session.id)?;
        envelope::save_json(&self.record_path("sessions", &session.id)?, &session)?;
        self.cache_saved_session(&session)?;
        Ok(session)
    }

    /// 原子移入回收目录，保留笔记、卡片和操作日志；重复删除可安全重试。
    pub(crate) fn delete_session(&self, id: &str) -> Result<(), CommandError> {
        let _guard = session_write_lock()?;
        let source = self.record_path("sessions", id)?;
        if !source.exists() {
            return Ok(());
        }
        let _ = self.session(id)?;
        let recycle = self.vault.safe_path(".quailcard/agent/.deleted-sessions")?;
        std::fs::create_dir_all(&recycle)?;
        let destination = self.vault.safe_path(&format!(
            ".quailcard/agent/.deleted-sessions/{id}-{}.json",
            Uuid::now_v7()
        ))?;
        self.invalidate_observation(id)?;
        std::fs::rename(source, destination)?;
        Ok(())
    }

    /// 前端只接收可见消息；协议续传项与工具原始配对只保留在后端。
    pub(crate) fn public_session(&self, id: &str) -> Result<AgentSession, CommandError> {
        Ok(crate::agent_public_history::project(self.session(id)?))
    }

    /// 记忆独立保存，不把整段聊天自动变成长期个人档案。
    pub(crate) fn save_memory(&self, content: &str) -> Result<AgentMemory, CommandError> {
        if content.chars().count() > 8000 {
            return Err(CommandError::validation("记忆最多 8000 字"));
        }
        let memory = AgentMemory {
            format_version: 1,
            content: content.to_string(),
        };
        let path = self.vault.safe_path(".quailcard/agent/.memory.json")?;
        let _: Option<AgentMemory> = envelope::load_json(&path, &CORRUPT)?;
        envelope::save_json(&path, &memory)?;
        Ok(memory)
    }

    /// 复习消息只保存界面快照，评分仍由已有复习命令负责。
    pub(crate) fn save_review(
        &self,
        session_id: &str,
        message_id: &str,
        progress: Value,
    ) -> Result<(), CommandError> {
        if progress.to_string().len() > 2 * 1024 * 1024 || !progress.is_object() {
            return Err(CommandError::validation("复习进度无效"));
        }
        let mut session = self.session(session_id)?;
        let message = session
            .messages
            .iter_mut()
            .find(|m| m.id == message_id && m.kind == "review")
            .ok_or_else(|| CommandError::validation("复习消息不存在"))?;
        message.data["progress"] = progress;
        self.save_session(&session)
    }
}

impl AgentRepository for AgentFiles {
    /// 大文件拒绝整篇加载，避免内存与模型上下文无限膨胀。
    fn read(&self, path: &str) -> Result<Value, CommandError> {
        let absolute = self.vault.agent_note_path(path)?;
        if std::fs::metadata(&absolute)?.len() > 256 * 1024 {
            return Err(CommandError::validation(
                "笔记超过 256 KiB，请先拆分后交给 Agent",
            ));
        }
        let content = std::fs::read_to_string(absolute)?;
        Ok(json!({"path":path,"content":content,"hash":changes::hash(&content)}))
    }

    /// 仅返回允许范围内的命中片段，不把整库正文发送给模型。
    fn search(&self, query: &str, scope: &[String]) -> Result<Value, CommandError> {
        let query = query.to_lowercase();
        let mut hits = Vec::new();
        let candidates = if scope.is_empty() {
            self.vault.scan()?
        } else {
            scope
                .iter()
                .map(|path| {
                    self.read(path).map(|note| {
                        (
                            path.clone(),
                            note["content"].as_str().unwrap_or("").to_string(),
                            0,
                        )
                    })
                })
                .collect::<Result<Vec<_>, _>>()?
        };
        for (path, content, _) in candidates {
            if (!scope.is_empty() && !scope.contains(&path))
                || self.vault.agent_note_path(&path).is_err()
            {
                continue;
            }
            if query.is_empty()
                || path.to_lowercase().contains(&query)
                || content.to_lowercase().contains(&query)
            {
                let snippet = content
                    .lines()
                    .find(|line| line.to_lowercase().contains(&query))
                    .unwrap_or("")
                    .chars()
                    .take(240)
                    .collect::<String>();
                hits.push(json!({"path":path,"snippet":snippet}));
            }
            if hits.len() >= 30 {
                break;
            }
        }
        Ok(json!({"notes":hits,"limit":30}))
    }

    /// 日志与文件的实际提交由同一存储子域实现。
    fn change(
        &self,
        id: &str,
        path: &str,
        content: &str,
        expected: Option<&str>,
    ) -> Result<AgentChange, CommandError> {
        self.apply_change(id, path, content, expected)
    }

    /// 每次保存写出当前完整格式，旧版本字段按默认值兼容。
    fn save_session(&self, session: &AgentSession) -> Result<(), CommandError> {
        let _guard = session_write_lock()?;
        let path = self.record_path("sessions", &session.id)?;
        let _: AgentSession = envelope::load_json(&path, &CORRUPT)?
            .ok_or_else(|| CommandError::new("AGENT_SESSION_MISSING", "会话不存在或已删除"))?;
        let mut session = session.clone();
        session.format_version = 1;
        self.invalidate_observation(&session.id)?;
        envelope::save_json(&path, &session)?;
        self.cache_saved_session(&session)
    }

    /// 父级授予子代理的写入范围必须经 vaultfs 净化且真实存在。
    fn validate_write_scope(&self, scope: &[String]) -> Result<Vec<String>, CommandError> {
        AgentFiles::validate_write_scope(self, scope)
    }

    /// 缺失记忆返回空记录，损坏文件禁止自动重置。
    fn memory(&self) -> Result<AgentMemory, CommandError> {
        Ok(envelope::load_json(
            &self.vault.safe_path(".quailcard/agent/.memory.json")?,
            &CORRUPT,
        )?
        .unwrap_or_default())
    }
}
