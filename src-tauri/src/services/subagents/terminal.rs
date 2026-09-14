use super::*;

/// 终态保存期间持有唯一冻结令牌；未提交即回滚，不把保存失败变成永久关闭。
#[must_use = "终态持久化成功后必须 commit，否则离开作用域将恢复准入"]
pub(crate) struct TerminalLease {
    manager: Arc<Subagents>,
    token: String,
    committed: bool,
}

impl Subagents {
    /// 域校验通过后再原子复验静默状态，关闭终态保存与迟到 send 之间的竞态窗口。
    pub(crate) fn seal_terminal(
        self: &Arc<Self>,
        caller: &str,
        require_quiet: bool,
    ) -> Result<TerminalLease, CommandError> {
        if caller != self.root_id {
            return Err(forbidden());
        }
        let mut tree = self.lock();
        self.ensure_open(&tree)?;
        if tree.terminal_seal.is_some() {
            return Err(terminal_sealed());
        }
        if require_quiet && messaging::pending(&tree, caller) {
            return Err(CommandError::new(
                "SUBAGENT_PENDING",
                "仍有未处理子消息或活动后代，不能完成目标",
            ));
        }
        let token = id();
        tree.terminal_seal = Some(token.clone());
        Ok(TerminalLease {
            manager: self.clone(),
            token,
            committed: false,
        })
    }

    /// 冻结只拒绝新派生与消息；根的最后模型总结仍受模型并发许可和停止保护。
    pub(super) fn ensure_work_admission(&self, tree: &Tree) -> Result<(), CommandError> {
        self.ensure_open(tree)?;
        if tree.terminal_seal.is_some() {
            return Err(terminal_sealed());
        }
        Ok(())
    }
}

impl TerminalLease {
    /// 保存成功后消耗租约，冻结延续到整树 shutdown，禁止迟到工作被静默启动后取消。
    pub(crate) fn commit(mut self) {
        self.committed = true;
    }
}

impl Drop for TerminalLease {
    /// 只撤销自己尚未提交的冻结，旧租约或关闭后的租约不能重新开放整树。
    fn drop(&mut self) {
        if self.committed {
            return;
        }
        let mut tree = self.manager.lock();
        if !tree.closed && tree.terminal_seal.as_deref() == Some(self.token.as_str()) {
            tree.terminal_seal = None;
        }
    }
}

/// 返回稳定安全码，前端可以明确提示当前目标已进入终态而不是误报发送成功。
fn terminal_sealed() -> CommandError {
    CommandError::new(
        "AGENT_GOAL_TERMINAL",
        "目标已进入终态，不能接收新的子 Agent 工作",
    )
}
