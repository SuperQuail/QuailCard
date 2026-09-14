use super::*;
use crate::services::agent_tasks::AgentTasks;
use futures_util::FutureExt;
use std::{future::Future, panic::AssertUnwindSafe, pin::Pin};

impl Subagents {
    /// 持锁登记 owned 任务，保证 shutdown 不会漏掉刚获准的启动。
    pub(super) fn launch(
        self: &Arc<Self>,
        tree: &mut Tree,
        target: &str,
    ) -> Result<(), CommandError> {
        self.ensure_work_admission(tree)?;
        if target == self.root_id {
            return Ok(());
        }
        let node = tree.nodes.get_mut(target).ok_or_else(forbidden)?;
        if node.running {
            return Ok(());
        }
        let Some(index) = node.inbox.iter().position(|m| m.kind == "message") else {
            return Ok(());
        };
        let execution_id = id();
        let (control, _) = AgentTasks::default().register(
            &self.root_control.owner,
            &self.root_control.root,
            &execution_id,
            target,
        )?;
        let message = node.inbox.remove(index).expect("message index checked");
        node.running = true;
        node.resume_after_cancel = false;
        node.control = Some(control.clone());
        let target = target.to_owned();
        // 已完成的任务已在内部处理 panic 与错误，不保留无限增长的历史句柄。
        tree.jobs.retain(|job| !job.is_finished());
        tree.jobs.push(tokio::spawn(
            self.clone().run_activation(target, message, control),
        ));
        Ok(())
    }

    /// 显式装箱打断递归派生的 Send 推导环；执行器 panic 也产生失败通知并释放 running。
    fn run_activation(
        self: Arc<Self>,
        target: String,
        message: ChildNotification,
        control: Arc<AgentControl>,
    ) -> Pin<Box<dyn Future<Output = ()> + Send + 'static>> {
        Box::pin(async move {
            let result = AssertUnwindSafe(async {
                let session = self.load_execution(&target)?;
                if control.is_cancelled() {
                    return Err(stopped());
                }
                let description = session.title.clone();
                self.executor
                    .execute(
                        ChildExecution {
                            session,
                            prompt: message.content,
                            description,
                            execution_id: control.snapshot().id,
                            message_id: message.message_id,
                        },
                        self.clone(),
                        control.clone(),
                    )
                    .await
            })
            .catch_unwind()
            .await
            .unwrap_or_else(|_| Err(CommandError::new("SUBAGENT_FAILED", "子 Agent 执行异常")));
            control.complete(result.as_ref().err());
            self.finish(&target, &control, result);
        })
    }

    /// 每次续聊重新加载最新历史，并同时复验不可变身份和权限，防止旧缓存覆盖新结果。
    fn load_execution(&self, target: &str) -> Result<AgentSession, CommandError> {
        let mut session = self.repo.load(target)?;
        let mut tree = self.lock();
        self.ensure_open(&tree)?;
        // 先取出父授权再借用子节点，避免同时持有两个可变借用。
        let (parent_write, parent_unlimited) =
            match tree.nodes.get(target).and_then(|node| node.parent.clone()) {
                Some(parent) => {
                    let parent = tree.nodes.get(&parent).ok_or_else(forbidden)?;
                    (parent.write_scope.clone(), parent.parent.is_none())
                }
                None => (Vec::new(), true),
            };
        let node = tree.nodes.get_mut(target).ok_or_else(forbidden)?;
        if session.id != target
            || session.parent_session_id != node.parent
            || session.delegation_depth != node.depth
        {
            return Err(forbidden());
        }
        let effective_scope = intersect(&session.selected_paths, &node.scope)?;
        if !same_scope(&effective_scope, &session.selected_paths) {
            return Err(forbidden());
        }
        // 写入授权随身份持久化：续聊只允许保持或收窄，绝不能被旧文件放大。
        let effective_write =
            restore_write_scope(&session.write_scope, &parent_write, parent_unlimited)?;
        session.selected_paths = effective_scope;
        session.write_scope = effective_write.clone();
        node.scope = session.selected_paths.clone();
        node.write_scope = effective_write;
        Ok(session)
    }

    /// 终态绑定执行身份；取消不自动续排队消息，迟到结果不得唤醒停止的根。
    fn finish(
        self: &Arc<Self>,
        target: &str,
        control: &Arc<AgentControl>,
        result: Result<String, CommandError>,
    ) {
        let mut tree = self.lock();
        let Some(node) = tree.nodes.get_mut(target) else {
            return;
        };
        if !node
            .control
            .as_ref()
            .is_some_and(|c| Arc::ptr_eq(c, control))
        {
            return;
        }
        node.running = false;
        let resume = !control.is_cancelled() || node.resume_after_cancel;
        let parent = node.parent.clone();
        if self.ensure_open(&tree).is_err() {
            return;
        }
        let (kind, content) = if control.is_cancelled() {
            ("cancelled", "子 Agent 当前轮次已中断".into())
        } else {
            match result {
                Ok(content) => ("completed", content),
                // 端口错误只暴露统一安全消息，不直接转发适配器诊断。
                Err(_) => ("failed", "子 Agent 执行或保存失败".into()),
            }
        };
        if let Some(parent) = parent.and_then(|p| tree.nodes.get_mut(&p)) {
            parent
                .inbox
                .retain(|m| m.kind == "message" || m.agent_id != target);
            parent.inbox.push_back(ChildNotification {
                message_id: id(),
                agent_id: target.into(),
                execution_id: control.snapshot().id,
                kind: kind.into(),
                content: bounded(content, self.limits.max_message_bytes),
            });
        }
        self.notify_ancestors(&tree, target);
        if resume && tree.terminal_seal.is_none() {
            // 仅接收过消息的空闲子开启下一轮，不因自己或后代自报完成而自动运行。
            if let Err(error) = self.launch(&mut tree, target) {
                if let Some(node) = tree.nodes.get_mut(target) {
                    node.inbox.push_back(ChildNotification {
                        message_id: id(),
                        agent_id: target.into(),
                        execution_id: control.snapshot().id,
                        kind: "failed".into(),
                        content: error.message,
                    });
                }
            }
        }
    }

    /// 宿主显式停止整树：先关准入，再取消并 join；不监听根的单消费者 cancelled。
    pub(crate) async fn shutdown(&self) {
        let _shutdown = self.shutdown_lock.lock().await;
        {
            let mut tree = self.lock();
            tree.closed = true;
            self.models.close();
            for (node_id, node) in &mut tree.nodes {
                if node_id != &self.root_id {
                    if let Some(control) = &node.control {
                        control.cancel();
                    }
                }
                node.inbox.clear();
                // 这里只释放宿主已挂起的 wait；closed 阻止任何自动续轮。
                node.changed.notify_waiters();
            }
        }
        let jobs = {
            let _admission = self.admission.lock().await;
            std::mem::take(&mut self.lock().jobs)
        };
        for job in jobs {
            // 执行器须响应独立 control 并持久化终态；join 不丢弃尚在进行的保存。
            if job.await.is_err() {
                // 内部异常标记失败而非伪装成人类取消。
                self.root_control.complete(Some(&CommandError::new(
                    "SUBAGENT_FAILED",
                    "子 Agent 收尾异常",
                )));
            }
        }
    }
}
