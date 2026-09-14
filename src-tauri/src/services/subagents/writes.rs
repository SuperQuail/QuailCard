//! 整树编辑器写入协调：查询与确认都绑定执行树内的真实控制身份。
use super::*;
use crate::services::agent_tasks::PendingWrite;

impl Subagents {
    /// 收集整树待保存写入：根优先，其后按深度与身份稳定排序。
    pub(crate) fn pending_writes(&self) -> Vec<PendingWrite> {
        let controls = {
            let tree = self.lock();
            if tree.closed || self.root_control.is_cancelled() {
                return Vec::new();
            }
            let mut ordered = tree
                .nodes
                .iter()
                .filter_map(|(id, node)| {
                    node.control
                        .as_ref()
                        .map(|control| (node.depth, id.clone(), control.clone()))
                })
                .collect::<Vec<_>>();
            ordered.sort_by(|left, right| left.0.cmp(&right.0).then_with(|| left.1.cmp(&right.1)));
            ordered
                .into_iter()
                .map(|(_, _, control)| control)
                .collect::<Vec<_>>()
        };
        // 状态锁外读取待写入字段，轮询路径不与工具执行抢锁。
        controls
            .iter()
            .filter_map(|control| control.pending_write())
            .collect()
    }

    /// 只确认本树执行身份自己的待写入；外来身份或过期操作一律拒绝。
    pub(crate) async fn acknowledge_write(
        &self,
        execution_id: &str,
        operation: &str,
    ) -> Result<(), CommandError> {
        let control = {
            let tree = self.lock();
            self.ensure_open(&tree)?;
            tree.nodes
                .values()
                .filter_map(|node| node.control.as_ref())
                .find(|control| control.is_execution(execution_id))
                .cloned()
                .ok_or_else(forbidden)?
        };
        // 确认会等待真实写入完成，因此不能在状态锁内等待。
        control.acknowledge(operation).await
    }
}
