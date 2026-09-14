use super::*;

impl Subagents {
    /// 测试便利入口：缺省派生没有写入授权，等价于只读子代理。
    #[cfg(test)]
    pub(crate) async fn spawn(
        self: &Arc<Self>,
        caller: &str,
        parent: &AgentSession,
        prompt: &str,
        description: &str,
        fork: bool,
        scope: Vec<String>,
    ) -> Result<String, CommandError> {
        self.spawn_granted(caller, parent, prompt, description, fork, scope, Vec::new())
            .await
    }

    /// 创建时一次性截取完整轮次历史；权限只认树内父边界，不信调用方旧快照。
    ///
    /// write_scope 只由父级在派生时提交并落盘；子代理自己无法修改，冷恢复也要重新复验。
    #[allow(clippy::too_many_arguments)]
    pub(crate) async fn spawn_granted(
        self: &Arc<Self>,
        caller: &str,
        parent: &AgentSession,
        prompt: &str,
        description: &str,
        fork: bool,
        scope: Vec<String>,
        write_scope: Vec<String>,
    ) -> Result<String, CommandError> {
        self.ensure_work_admission(&self.lock())?;
        self.validate_text(prompt)?;
        self.validate_text(description)?;
        let _admission = self.admission.lock().await;
        let (depth, parent_scope, granted_write) = {
            let tree = self.lock();
            self.ensure_work_admission(&tree)?;
            let node = self.caller(&tree, caller)?;
            if parent.id != caller || node.control.as_ref().is_some_and(|c| c.is_cancelled()) {
                return Err(forbidden());
            }
            let depth = node.depth.checked_add(1).ok_or_else(forbidden)?;
            self.check_capacity(&tree, depth)?;
            // 只有根的空范围表示整库；子节点为空即只读，不能自行再授予任何路径。
            let granted =
                grant_write_scope(&write_scope, &node.write_scope, node.parent.is_none())?;
            (depth, node.scope.clone(), granted)
        };
        if !parent_scope.is_empty() && scope.iter().any(|p| !parent_scope.contains(p)) {
            return Err(forbidden());
        }
        let effective_scope = intersect(&scope, &parent_scope)?;
        if fork && !same_scope(&effective_scope, &parent_scope) {
            return Err(forbidden());
        }
        let mut child = AgentSession {
            id: id(),
            format_version: parent.format_version,
            title: description.into(),
            parent_session_id: Some(caller.into()),
            delegation_depth: depth,
            selected_paths: effective_scope,
            write_scope: granted_write,
            ..Default::default()
        };
        if fork {
            child.messages = parent
                .messages
                .iter()
                .take(parent.completed_message_count.min(parent.messages.len()))
                .filter(|m| !matches!(m.kind.as_str(), "plan" | "goal" | "running"))
                .cloned()
                .collect();
            child.completed_message_count = child.messages.len();
        }
        self.repo.create(&child)?;
        let child_id = child.id.clone();
        let mut tree = self.lock();
        self.ensure_work_admission(&tree)?;
        if self
            .caller(&tree, caller)?
            .control
            .as_ref()
            .is_some_and(|c| c.is_cancelled())
        {
            return Err(forbidden());
        }
        tree.nodes
            .insert(child_id.clone(), Node::from_session(&child));
        self.enqueue(&mut tree, caller, &child_id, prompt)?;
        self.launch(&mut tree, &child_id)?;
        Ok(child_id)
    }

    /// 身份额度包含空闲孩子；默认深度从根零层起算，不允许溢出绕过。
    fn check_capacity(&self, tree: &Tree, depth: u32) -> Result<(), CommandError> {
        if depth > self.limits.max_depth
            || tree.nodes.len().saturating_sub(1) >= self.limits.max_agents
        {
            Err(CommandError::new(
                "SUBAGENT_LIMIT",
                "子 Agent 深度或数量已达限制",
            ))
        } else {
            Ok(())
        }
    }

    /// 只允许冷加载 caller 的直接子，重新相交授权；绝不读取旧父快照再保存。
    pub(super) fn restore(&self, caller: &str, target: &str) -> Result<(), CommandError> {
        let (depth, parent_scope, parent_write, parent_unlimited) = {
            let tree = self.lock();
            self.ensure_open(&tree)?;
            let parent = self.caller(&tree, caller)?;
            (
                parent.depth,
                parent.scope.clone(),
                parent.write_scope.clone(),
                parent.parent.is_none(),
            )
        };
        let mut child = self.repo.load(target)?;
        if child.id != target
            || child.parent_session_id.as_deref() != Some(caller)
            || depth.checked_add(1) != Some(child.delegation_depth)
        {
            return Err(forbidden());
        }
        let effective_scope = intersect(&child.selected_paths, &parent_scope)?;
        if !same_scope(&effective_scope, &child.selected_paths) {
            return Err(forbidden());
        }
        // 写入授权不能被磁盘上的旧文件放大：恢复后仍必须落在父范围内。
        let effective_write =
            restore_write_scope(&child.write_scope, &parent_write, parent_unlimited)?;
        child.selected_paths = effective_scope;
        child.write_scope = effective_write;
        let mut tree = self.lock();
        self.ensure_open(&tree)?;
        self.check_capacity(&tree, child.delegation_depth)?;
        tree.nodes.insert(target.into(), Node::from_session(&child));
        Ok(())
    }
}
