//! 只读历史版本缓存：应用内写入强制换代，外部普通修改由高精度文件指纹发现。
//! 不以秒级 updated_at 或运行序号判断历史；驱逐/重启只会保守地重新发送。
//! 这不是磁盘防篡改校验：外部刻意保留全部指纹的同长原地修改不保证识别。
use super::*;
use std::{
    collections::HashMap,
    path::PathBuf,
    sync::{Mutex, OnceLock},
    time::SystemTime,
};

/// 缓存只保留鉴权身份，不留历史、供应商协议或凭据。
#[derive(Clone)]
pub(crate) struct SessionIdentity {
    pub id: String,
    pub parent: Option<String>,
    pub depth: u32,
}

/// 纳秒级时间与长度共同检测外部修改；应用写入不依赖文件系统时间粒度。
#[derive(Clone, PartialEq)]
struct Fingerprint {
    modified: SystemTime,
    created: Option<SystemTime>,
    len: u64,
}

#[derive(Clone)]
struct Entry {
    fingerprint: Option<Fingerprint>,
    revision: String,
    identity: SessionIdentity,
}
static CACHE: OnceLock<Mutex<HashMap<PathBuf, Entry>>> = OnceLock::new();
#[cfg(test)]
thread_local! { static READS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) }; }

/// 测试计数仅在当前线程累计正文加载，避免并行测试相互干扰。
#[cfg(test)]
pub(super) fn record_read() {
    READS.with(|reads| reads.set(reads.get() + 1));
}

/// 测试用计数器验证授权和历史未变分支没有隐藏的全量读取。
#[cfg(test)]
impl AgentFiles {
    /// 读取当前线程累计正文次数，测试热路径时不受其他测试线程影响。
    pub(crate) fn session_read_count() -> usize {
        READS.with(|reads| reads.get())
    }
}

/// 缓存锁从不跨读盘或会话写锁获取，避免锁顺序倒置。
fn cache() -> std::sync::MutexGuard<'static, HashMap<PathBuf, Entry>> {
    CACHE
        .get_or_init(Default::default)
        .lock()
        .unwrap_or_else(|e| e.into_inner())
}

/// 无法取得修改时间时禁用缓存；应用写入另有强制失效，不依赖系统时间粒度。
fn fingerprint(path: &Path) -> Result<Option<Fingerprint>, CommandError> {
    let metadata = std::fs::metadata(path).map_err(|error| {
        if error.kind() == std::io::ErrorKind::NotFound {
            CommandError::new("AGENT_SESSION_MISSING", "会话不存在")
        } else {
            CommandError::new("FILE_ERROR", "会话文件暂时无法读取")
        }
    })?;
    Ok(metadata.modified().ok().map(|modified| Fingerprint {
        modified,
        created: metadata.created().ok(),
        len: metadata.len(),
    }))
}

/// 已有字段的语义不变；版本令牌与 Vault 安全路径隔离且不携带路径信息。
fn entry(session: &AgentSession, fingerprint: Option<Fingerprint>) -> Entry {
    Entry {
        fingerprint,
        revision: format!("v1:{}", Uuid::now_v7()),
        identity: SessionIdentity {
            id: session.id.clone(),
            parent: session.parent_session_id.clone(),
            depth: session.delegation_depth,
        },
    }
}

/// 有界缓存避免打开大量 Vault 后无限积累；清空只导致保守刷新。
fn insert(path: PathBuf, entry: Entry) {
    let mut cache = cache();
    if cache.len() >= 512 {
        cache.clear();
    }
    cache.insert(path, entry);
}

impl AgentFiles {
    /// 写入前移除旧代；即使写入失败，也不会复用状态不确定的鉴权缓存。
    pub(super) fn invalidate_observation(&self, id: &str) -> Result<(), CommandError> {
        let path = self.record_path("sessions", id)?;
        cache().remove(&path);
        Ok(())
    }

    /// 必须在会话写锁内于原子提交之后发布；同秒同长度的保存也获得全新版本。
    pub(super) fn cache_saved_session(&self, session: &AgentSession) -> Result<(), CommandError> {
        let path = self.record_path("sessions", &session.id)?;
        let stamp = fingerprint(&path)?;
        insert(path, entry(session, stamp));
        Ok(())
    }

    /// 热路径只检查文件元数据；身份变化或损坏必须重新验证信封后才能进入缓存。
    pub(crate) fn session_identity(&self, id: &str) -> Result<SessionIdentity, CommandError> {
        let _guard = session_write_lock()?;
        let path = self.record_path("sessions", id)?;
        let stamp = fingerprint(&path)?;
        if let Some(hit) = cache()
            .get(&path)
            .filter(|hit| stamp.is_some() && hit.fingerprint == stamp)
            .cloned()
        {
            return Ok(hit.identity);
        }
        let session = self.session(id)?;
        let fresh = entry(&session, stamp);
        let identity = fresh.identity.clone();
        insert(path, fresh);
        Ok(identity)
    }

    /// 已知版本命中时不读正文、不解析 JSON、不生成工具 DTO；原子写锁保证采样一致。
    pub(crate) fn observe_history(
        &self,
        id: &str,
        known: Option<&str>,
    ) -> Result<(String, Option<AgentSession>), CommandError> {
        let _guard = session_write_lock()?;
        let path = self.record_path("sessions", id)?;
        let stamp = fingerprint(&path)?;
        let hit = cache()
            .get(&path)
            .filter(|hit| stamp.is_some() && hit.fingerprint == stamp)
            .cloned();
        if let Some(hit) = &hit {
            if known == Some(hit.revision.as_str()) {
                return Ok((hit.revision.clone(), None));
            }
        }
        let session = self.session(id)?;
        let fresh = hit.unwrap_or_else(|| entry(&session, stamp));
        let revision = fresh.revision.clone();
        insert(path, fresh);
        Ok((
            revision,
            Some(crate::agent_public_history::project(session)),
        ))
    }
}

#[cfg(test)]
#[path = "agent_observation_tests.rs"]
mod tests;
