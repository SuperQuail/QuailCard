use super::*;

impl Subagents {
    /// 接收确认不是执行答案；工作中的消息留到安全步边界由 executor drain。
    pub(crate) async fn send(
        self: &Arc<Self>,
        caller: &str,
        target: &str,
        message: &str,
    ) -> Result<String, CommandError> {
        self.ensure_work_admission(&self.lock())?;
        self.validate_text(message)?;
        let _admission = self.admission.lock().await;
        let missing = {
            let tree = self.lock();
            self.ensure_open(&tree)?;
            let source = self.caller(&tree, caller)?;
            if source.control.as_ref().is_some_and(|c| c.is_cancelled()) {
                return Err(forbidden());
            }
            !tree.nodes.contains_key(target)
        };
        if missing {
            self.restore(caller, target)?;
        }
        let mut tree = self.lock();
        self.ensure_work_admission(&tree)?;
        let source = self.caller(&tree, caller)?;
        if source.control.as_ref().is_some_and(|c| c.is_cancelled()) {
            return Err(forbidden());
        }
        let receiver = tree.nodes.get(target).ok_or_else(forbidden)?;
        if source.parent.as_deref() != Some(target) && receiver.parent.as_deref() != Some(caller) {
            return Err(forbidden());
        }
        let message_id = self.enqueue(&mut tree, caller, target, message)?;
        let receiver = tree.nodes.get_mut(target).ok_or_else(forbidden)?;
        if receiver.control.as_ref().is_some_and(|c| c.is_cancelled()) {
            receiver.resume_after_cancel = true;
        }
        self.launch(&mut tree, target)?;
        Ok(message_id)
    }

    /// 消息队列有界且超限显式失败；完成报告另保留每个直接子最新的一份。
    pub(super) fn enqueue(
        &self,
        tree: &mut Tree,
        caller: &str,
        target: &str,
        content: &str,
    ) -> Result<String, CommandError> {
        let execution_id = tree
            .nodes
            .get(caller)
            .and_then(|n| n.control.as_ref())
            .map(|c| c.snapshot().id)
            .unwrap_or_default();
        let node = tree.nodes.get_mut(target).ok_or_else(forbidden)?;
        if node.inbox.iter().filter(|m| m.kind == "message").count() >= self.limits.max_messages {
            return Err(CommandError::new(
                "SUBAGENT_MAILBOX_FULL",
                "子 Agent 消息队列已满",
            ));
        }
        let message_id = id();
        node.inbox.push_back(ChildNotification {
            message_id: message_id.clone(),
            agent_id: caller.into(),
            execution_id,
            kind: "message".into(),
            content: content.into(),
        });
        node.changed.notify_waiters();
        Ok(message_id)
    }

    /// 任意祖先可中断驻留后代的当前轮次；不波及目标自己的后代或排队消息。
    pub(crate) fn interrupt(&self, caller: &str, target: &str) -> Result<(), CommandError> {
        let mut tree = self.lock();
        self.ensure_open(&tree)?;
        self.caller(&tree, caller)?;
        if !is_descendant(&tree, caller, target) {
            return Err(forbidden());
        }
        let node = tree.nodes.get_mut(target).ok_or_else(forbidden)?;
        node.resume_after_cancel = false;
        if node.running {
            if let Some(control) = &node.control {
                control.cancel();
            }
        }
        // 唤醒管理器自己的等待点；不竞争 AgentControl 的单消费者取消通知。
        node.changed.notify_waiters();
        Ok(())
    }

    /// 安全步边界只查看通知；宿主保存候选历史成功后再 ack，失败不丢失消息。
    pub(crate) fn peek(&self, caller: &str) -> Vec<ChildNotification> {
        let tree = self.lock();
        if self.ensure_open(&tree).is_err() {
            return vec![];
        }
        let Some(node) = tree.nodes.get(caller) else {
            return vec![];
        };
        if node.control.as_ref().is_some_and(|c| c.is_cancelled()) {
            return vec![];
        }
        node.inbox.iter().cloned().collect()
    }

    /// 只确认已持久化的精确消息身份；保存期间新到达或替换的完成通知必须保留。
    pub(crate) fn ack(&self, caller: &str, ids: &[String]) {
        let mut tree = self.lock();
        if let Some(node) = tree.nodes.get_mut(caller) {
            node.inbox
                .retain(|message| !ids.contains(&message.message_id));
        }
    }

    /// 仅供既有内存契约测试使用；生产 runner 必须分开 peek、持久化、ack。
    #[cfg(test)]
    pub(crate) fn drain(&self, caller: &str) -> Vec<ChildNotification> {
        let messages = self.peek(caller);
        let ids: Vec<_> = messages
            .iter()
            .map(|message| message.message_id.clone())
            .collect();
        self.ack(caller, &ids);
        messages
    }

    /// 供宿主判断是否需要等待：通知或任意活跃后代都阻止过早完成。
    pub(crate) fn has_pending(&self, caller: &str) -> bool {
        let tree = self.lock();
        if self.ensure_open(&tree).is_err() {
            return false;
        }
        pending(&tree, caller)
    }

    /// 注册事件先于检查谓词，避免结果恰在挂起前到达而丢失唤醒；无模型轮询。
    pub(crate) async fn wait(&self, caller: &str) {
        let changed = {
            let tree = self.lock();
            let Some(node) = tree.nodes.get(caller) else {
                return;
            };
            node.changed.clone()
        };
        loop {
            let event = changed.notified();
            tokio::pin!(event);
            event.as_mut().enable();
            {
                let tree = self.lock();
                if self.ensure_open(&tree).is_err() {
                    return;
                }
                let Some(node) = tree.nodes.get(caller) else {
                    return;
                };
                if node.control.as_ref().is_some_and(|c| c.is_cancelled())
                    || !node.inbox.is_empty()
                    || !pending(&tree, caller)
                {
                    return;
                }
            }
            event.await;
        }
    }

    /// 收尾变化向祖先传播事件，但没有消息的祖先仍按谓词等待剩余工作。
    pub(super) fn notify_ancestors(&self, tree: &Tree, target: &str) {
        if self.ensure_open(tree).is_err() {
            return;
        }
        let mut current = target;
        while let Some(node) = tree.nodes.get(current) {
            node.changed.notify_waiters();
            let Some(parent) = node.parent.as_deref() else {
                break;
            };
            current = parent;
        }
    }
}

/// 权限只沿驻留树的权威父关系追溯，禁止兄弟、自身及根反向控制。
fn is_descendant(tree: &Tree, ancestor: &str, target: &str) -> bool {
    let mut current = target;
    while let Some(parent) = tree.nodes.get(current).and_then(|n| n.parent.as_deref()) {
        if parent == ancestor {
            return true;
        }
        current = parent;
    }
    false
}

/// 排队但已中断的消息不能让父任务永久等待；等待由新 send 明确恢复。
pub(super) fn pending(tree: &Tree, caller: &str) -> bool {
    let Some(node) = tree.nodes.get(caller) else {
        return false;
    };
    if node.control.as_ref().is_some_and(|c| c.is_cancelled()) {
        return false;
    }
    !node.inbox.is_empty()
        || tree
            .nodes
            .iter()
            .any(|(id, n)| n.running && is_descendant(tree, caller, id))
}
