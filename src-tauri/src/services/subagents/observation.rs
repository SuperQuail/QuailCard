//! 只读运行观察不准入工作，也不把磁盘身份冷恢复为驻留节点。
use super::*;
use crate::agent_models::AgentRun;
use std::collections::BTreeSet;

impl Subagents {
    /// 已驻留目标须沿真实父链属于 caller；未知冷身份返回空，由组合层先验磁盘父链。
    pub(crate) fn observe_run(
        &self,
        caller: &str,
        target: &str,
    ) -> Result<Option<AgentRun>, CommandError> {
        let tree = self.lock();
        self.caller(&tree, caller)?;
        if target == caller {
            return Err(forbidden());
        }
        let Some(node) = tree.nodes.get(target) else {
            return Ok(None);
        };
        validate_descendant(&tree, caller, target)?;
        if tree.closed || self.root_control.is_cancelled() {
            return Ok(None);
        }
        Ok(node.control.as_ref().map(|control| control.snapshot()))
    }
}

/// 有界内存父链同时验证层级与循环，不能借损坏节点跨兄弟或反向观察祖先。
fn validate_descendant(tree: &Tree, caller: &str, target: &str) -> Result<(), CommandError> {
    let mut current = target;
    let mut seen = BTreeSet::new();
    while let Some(node) = tree.nodes.get(current) {
        if !seen.insert(current) {
            return Err(forbidden());
        }
        let parent_id = node.parent.as_deref().ok_or_else(forbidden)?;
        let parent = tree.nodes.get(parent_id).ok_or_else(forbidden)?;
        if parent.depth.checked_add(1) != Some(node.depth) {
            return Err(forbidden());
        }
        if parent_id == caller {
            return Ok(());
        }
        current = parent_id;
    }
    Err(forbidden())
}
