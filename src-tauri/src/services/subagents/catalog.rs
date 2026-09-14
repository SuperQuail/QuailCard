//! 一次刷新一份父索引；查询验证身份关系，读写范围准入留给恢复与执行。
use super::*;
use std::collections::BTreeSet;

/// 快照只保存目录元数据，不保留模型历史或协议载荷。
#[derive(Clone)]
pub(crate) struct ChildRecord {
    info: ChildInfo,
    root: bool,
}

impl ChildRecord {
    /// 查询仅保留身份与展示元数据，不因父授权后来收窄而隐藏历史子会话。
    pub(crate) fn from_session(session: &AgentSession) -> Self {
        Self {
            info: ChildInfo {
                agent_id: session.id.clone(),
                parent_session_id: session.parent_session_id.clone().unwrap_or_default(),
                delegation_depth: session.delegation_depth,
                description: session.title.clone(),
                write_scope: session.write_scope.clone(),
                status: "ready".into(),
            },
            root: session.parent_session_id.is_none(),
        }
    }

    /// 内存状态只补充驻留事实；权限仍来自管理器内已验证的节点。
    fn from_node(id: &str, node: &Node) -> Self {
        Self {
            info: ChildInfo {
                agent_id: id.into(),
                parent_session_id: node.parent.clone().unwrap_or_default(),
                delegation_depth: node.depth,
                description: node.description.clone(),
                write_scope: node.write_scope.clone(),
                status: if node.running { "running" } else { "idle" }.into(),
            },
            root: node.parent.is_none(),
        }
    }

    /// 查询只验证连续父边与深度；列出旧授权不表示通过当前恢复或执行准入。
    fn validate_child(&self, child: &Self) -> Result<(), CommandError> {
        if child.root
            || child.info.parent_session_id != self.info.agent_id
            || self.info.delegation_depth.checked_add(1) != Some(child.info.delegation_depth)
        {
            return Err(forbidden());
        }
        Ok(())
    }
}

/// 单次调用拥有的父关系索引；不跨刷新缓存，防止错过新建或删除身份。
#[derive(Default)]
pub(crate) struct ChildSnapshot {
    children: BTreeMap<String, BTreeMap<String, ChildRecord>>,
}

impl ChildSnapshot {
    /// 重复身份整体拒绝，不能由枚举顺序决定哪个父关系生效。
    pub(crate) fn from_records(
        records: impl IntoIterator<Item = ChildRecord>,
    ) -> Result<Self, CommandError> {
        let mut snapshot = Self::default();
        let mut seen = BTreeSet::new();
        for record in records {
            if record.info.agent_id.is_empty() || !seen.insert(record.info.agent_id.clone()) {
                return Err(forbidden());
            }
            if !record.root {
                snapshot
                    .children
                    .entry(record.info.parent_session_id.clone())
                    .or_default()
                    .insert(record.info.agent_id.clone(), record);
            }
        }
        Ok(snapshot)
    }

    /// 消费当前父的桶，遍历每个节点不再筛选全库或复制其他分支。
    fn take(&mut self, parent: &str) -> BTreeMap<String, ChildRecord> {
        self.children.remove(parent).unwrap_or_default()
    }
}

/// 冷列表入口要求真实根快照；组合层仍须验证窗口、Vault 与请求身份。
pub(crate) fn stored_children(
    repo: &dyn ChildRepository,
    root: &AgentSession,
) -> Result<Vec<ChildInfo>, CommandError> {
    if root.parent_session_id.is_some() || root.delegation_depth != 0 || root.id.is_empty() {
        return Err(forbidden());
    }
    collect(
        repo,
        ChildRecord::from_session(root),
        ChildSnapshot::default(),
        true,
        u32::MAX,
    )
}

impl Subagents {
    /// 工具列表保留取消契约；一次刷新仅取一份目录快照。
    pub(crate) fn list(
        &self,
        caller: &str,
        descendants: bool,
    ) -> Result<Vec<ChildInfo>, CommandError> {
        self.ensure_open(&self.lock())?;
        let output = self.collect_catalog(caller, descendants, self.limits.max_depth)?;
        self.ensure_open(&self.lock())?;
        Ok(output)
    }

    /// UI 观察不因扫描期间取消而二次读盘；结束树只降级状态，绝不准入工作。
    pub(crate) fn observe_children(&self, caller: &str) -> Result<Vec<ChildInfo>, CommandError> {
        let mut output = self.collect_catalog(caller, true, u32::MAX)?;
        let tree = self.lock();
        if tree.closed || self.root_control.is_cancelled() {
            for child in &mut output {
                child.status = "ready".into();
            }
        }
        Ok(output)
    }

    /// 状态锁内只采集授权元数据，所有仓库访问都在锁外且不触发冷恢复。
    fn collect_catalog(
        &self,
        caller: &str,
        descendants: bool,
        max_depth: u32,
    ) -> Result<Vec<ChildInfo>, CommandError> {
        let (root, live) = {
            let tree = self.lock();
            let root = ChildRecord::from_node(caller, self.caller(&tree, caller)?);
            let live = ChildSnapshot::from_records(
                tree.nodes
                    .iter()
                    .map(|(id, node)| ChildRecord::from_node(id, node)),
            )?;
            (root, live)
        };
        collect(self.repo.as_ref(), root, live, descendants, max_depth)
    }
}

/// 深度优先前序稳定排序；兼容旧仓库回退，但生产快照路径绝不逐节点调用 list。
fn collect(
    repo: &dyn ChildRepository,
    root: ChildRecord,
    mut live: ChildSnapshot,
    descendants: bool,
    max_depth: u32,
) -> Result<Vec<ChildInfo>, CommandError> {
    let mut snapshot = repo.snapshot()?;
    let root_id = root.info.agent_id.clone();
    let mut visited = BTreeSet::from([root_id.clone()]);
    let mut output = Vec::new();
    let mut pending = vec![root];
    while let Some(parent) = pending.pop() {
        let parent_id = &parent.info.agent_id;
        if parent_id != &root_id {
            output.push(parent.info.clone());
            if !descendants || parent.info.delegation_depth >= max_depth {
                continue;
            }
        }
        let mut children = match &mut snapshot {
            Some(snapshot) => snapshot.take(parent_id),
            None => fallback_children(repo, &parent)?,
        };
        // 磁盘边即使被驻留状态覆盖也要验证，不得借覆盖隐藏伪造父链。
        for child in children.values() {
            parent.validate_child(child)?;
        }
        children.extend(live.take(parent_id));
        let mut nested = Vec::new();
        for (id, child) in children {
            parent.validate_child(&child)?;
            if !visited.insert(id) || visited.len() > 256 {
                return Err(forbidden());
            }
            nested.push(child);
        }
        pending.extend(nested.into_iter().rev());
    }
    Ok(output)
}

/// 默认适配器只为既有假仓库提供兼容；错误父边和重复身份仍不能被索引掩盖。
fn fallback_children(
    repo: &dyn ChildRepository,
    parent: &ChildRecord,
) -> Result<BTreeMap<String, ChildRecord>, CommandError> {
    let mut children = BTreeMap::new();
    for session in repo.list(&parent.info.agent_id)? {
        let child = ChildRecord::from_session(&session);
        parent.validate_child(&child)?;
        if children.insert(session.id, child).is_some() {
            return Err(forbidden());
        }
    }
    Ok(children)
}
