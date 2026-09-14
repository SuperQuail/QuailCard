//! 单次模型请求与工具批次的执行；生命周期与续轮由外层持有。
use super::*;
use crate::services::agent_write_scope::WriteAuthority;

impl TurnStep for AgentTurn<'_> {
    /// 记录文本、执行工具并更新会话；结束原因由统一骨架解读。
    fn step<'a>(&'a mut self, step: u32) -> TurnFuture<'a, TurnOutcome> {
        Box::pin(async move {
            if self.control.is_cancelled() {
                return self.cancel().map(|_| TurnOutcome::Finish);
            }
            // 业务终态已提交，无须再付费请求总结，也不能被总结网络失败推翻。
            if self.terminal_goal || self.waiting_user {
                return self.after_response().await;
            }
            if !self.terminal_goal {
                self.receive_children()?;
            }
            let control = self.control;
            let repository = self.ports.repository;
            let model = self.ports.model;
            let message_id = uuid::Uuid::now_v7().to_string();
            let reasoning_message_id = uuid::Uuid::now_v7().to_string();
            control.emit(AgentEvent::StepStart {
                step,
                message_id: message_id.clone(),
                reasoning_message_id: reasoning_message_id.clone(),
            });
            let delta = |text: &str| {
                control.emit(AgentEvent::TextDelta {
                    text: text.to_string(),
                });
            };
            // 推理既实时展示也落盘，便于复盘当时的判断过程；闭包要 Send+Sync，用互斥量累计。
            let thought = std::sync::Mutex::new(String::new());
            let reasoning = |text: &str| {
                control.emit(AgentEvent::ReasoningDelta {
                    text: text.to_string(),
                });
                if let Ok(mut value) = thought.lock() {
                    value.push_str(text);
                }
            };
            // 拆卡模式下工具集随模式切换，声明在每次请求前重新生成。
            let definitions = self.tool_definitions();
            // 模型流等待也响应取消，收尾由骨架在下一轮入口执行。
            let _permit = if let Some(tree) = &self.tree {
                Some(tokio::select! { biased;
                    _ = control.cancelled() => return self.cancel().map(|_| TurnOutcome::Finish),
                    permit = tree.acquire_model() => permit?,
                })
            } else {
                None
            };
            let reply = tokio::select! { biased;
                _ = control.cancelled() => return self.cancel().map(|_| TurnOutcome::Finish),
                reply = model.call(&self.system, &self.history, &definitions, &delta, &reasoning) => reply?,
            };
            drop(_permit);
            // 推理排在正文之前，历史顺序与展示顺序一致。
            let thought_text = thought
                .lock()
                .map(|value| value.clone())
                .unwrap_or_default();
            if !thought_text.is_empty() {
                let mut message = tools::block("reasoning", &thought_text, Value::Null);
                message.id = reasoning_message_id.clone();
                self.session.messages.push(message);
            }
            if !reply.text.is_empty() {
                let mut message = tools::block("text", &reply.text, Value::Null);
                message.id = message_id.clone();
                self.session.messages.push(message);
            }
            repository.save_session(self.session)?;
            control.emit(AgentEvent::TextCommitted {
                message_id: message_id.clone(),
            });
            if reply.calls.is_empty() {
                if self.generation.is_some() {
                    self.history.push(reply.replay);
                    self.history.push(json!({"role":"user","content":"拆卡尚未完成，请继续调用生成工具；不要只返回文字。"}));
                    return Ok(TurnOutcome::Stalled);
                }
                if reply.text.trim().is_empty() {
                    return Err(CommandError::new(
                        "AGENT_EMPTY_RESPONSE",
                        "模型没有返回内容，请重试",
                    ));
                }
                self.history.push(reply.replay);
                return self.after_response().await;
            }
            if self.terminal_goal {
                return Err(CommandError::validation("目标已经结束，只能输出总结"));
            }
            self.history.push(reply.replay.clone());
            // 保留完整工具配对供审计，恢复时不会自动重放工具。
            let pending = reply.calls.iter().map(|call| json!({"role":"tool","tool_call_id":call.id,"content":"{\"ok\":false,\"status\":\"notExecuted\"}"})).collect::<Vec<_>>();
            let receipt_calls = reply
                .calls
                .iter()
                // 中立展示只保存类型白名单参数；禁止复制 URL、正文或供应商输入。
                .map(|call| {
                    json!({"id":call.id,"name":call.name,
                    "arguments":crate::agent_public_history::safe_arguments(&call.arguments)})
                })
                .collect::<Vec<_>>();
            let mut exchange =
                json!({"assistant":reply.replay,"results":pending,"calls":receipt_calls});
            let exchange_index = self.session.messages.len();
            self.session
                .messages
                .push(tools::block("exchange", "工具记录", exchange.clone()));
            repository.save_session(self.session)?;
            let mut progress = false;
            let mut pause = false;
            let mut generation_finish = None;
            // 拆卡模式：同一条响应里的生成调用合成一批交给生成管线，
            // 这样 plan→lookup→emit→finish 的执行顺序与词典延迟规则都保持生效。
            let mut generated_calls = std::collections::HashMap::new();
            if let Some(mode) = self.generation.as_mut() {
                let calls = reply
                    .calls
                    .iter()
                    .filter(|call| mode.tools.description(&call.name).is_some())
                    .cloned()
                    .collect::<Vec<_>>();
                if !calls.is_empty() {
                    let (contents, progressed, finished) =
                        run_generation_batch(self.ports.dictionary, mode, &calls, control).await;
                    generated_calls = contents;
                    progress |= progressed;
                    generation_finish = finished;
                    // 拆卡阶段接进 Agent 快照，界面能看到「规划/校验 + 已生成 N 张」。
                    let phase = mode.control.phase();
                    let generated = mode.prepared.session.generated();
                    control.update(|state| {
                        state.phase = format!("拆卡：{phase}（已生成 {generated} 张）");
                    });
                }
            }
            for (call_index, call) in reply.calls.into_iter().enumerate() {
                // 生成调用已由批处理执行，这里只按 call id 把结果回填交换块与历史。
                if let Some(content) = generated_calls.remove(&call.id) {
                    let value =
                        serde_json::from_str::<Value>(&content).unwrap_or(Value::String(content));
                    let result_message = tools::tool_message(&call.id, &value, &[]);
                    exchange["results"][call_index] = result_message.clone();
                    self.session.messages[exchange_index].data = exchange.clone();
                    self.history.push(result_message);
                    repository.save_session(self.session)?;
                    continue;
                }
                if control.is_cancelled() {
                    return self.cancel().map(|_| TurnOutcome::Finish);
                }
                let operation = uuid::Uuid::now_v7().to_string();
                // 写入授权在派生后不可变；取局部快照，避免与运行态工具的可变借用冲突。
                let write_scope = self.session.write_scope.clone();
                let authority = if self.session.parent_session_id.is_some() {
                    WriteAuthority::Scoped(&write_scope)
                } else {
                    WriteAuthority::Root
                };
                let context = tools::ToolContext {
                    repository,
                    learning: self.ports.learning,
                    video: self.ports.video,
                    dictionary: self.ports.dictionary,
                    cards: self.ports.cards,
                    scope: &self.input.selected_paths,
                    write_scope: authority,
                    operation: &operation,
                };
                let tool = self.registered.iter().find(|t| t.spec.name == call.name);
                let runtime_tool = self
                    .runtime_tools
                    .iter()
                    .find(|t| t.spec.name == call.name)
                    .cloned();
                let result = if pause || self.terminal_goal || self.waiting_user {
                    Err(CommandError::new("AGENT_WAITING", "请等待用户完成当前交互"))
                } else if call.name == "generate_cards" && self.generation.is_some() {
                    Err(CommandError::new(
                        "GENERATION_ACTIVE",
                        "请先完成当前拆卡任务，不要重新开始",
                    ))
                } else if let Some(tool) = runtime_tool {
                    runtime_tools::invoke(&tool, self, &call.arguments).await
                } else if let Some(tool) = tool {
                    match tools::validate(tool, &call.arguments) {
                        Err(error) => Err(error),
                        Ok(()) => {
                            control.emit(AgentEvent::ToolStart {
                                name: call.name.clone(),
                                label: tool.spec.description,
                            });
                            if tool.spec.writes() {
                                // 日志身份先保存，崩溃后仍能从会话找到待核对改动。
                                self.session.messages.push(tools::block(
                                    "change",
                                    "笔记改动",
                                    json!({"changeId":operation,"path":call.arguments["path"]}),
                                ));
                                repository.save_session(self.session)?;
                                // 写前继续等待编辑器确认；用户取消仍能唤醒等待并清除标记。
                                control
                                    .confirm_write(
                                        call.arguments["path"].as_str().unwrap_or(""),
                                        &operation,
                                    )
                                    .await?;
                            }
                            let result = if let Some(handler) = tool.async_handler {
                                super::tool_progress::wait(
                                    handler(&context, &call.arguments),
                                    context.video,
                                    control,
                                    tool.spec.description,
                                )
                                .await
                            } else {
                                (tool.handler)(&context, &call.arguments)
                            };
                            if tool.spec.writes() {
                                control.update(|s| {
                                    s.pending_write = None;
                                    s.pending_write_id = None;
                                });
                            }
                            result
                        }
                    }
                } else {
                    Err(CommandError::validation("模型调用了未注册工具"))
                };
                let (value, images) = match result {
                    Ok(outcome) => {
                        // 重复调用仍然执行（文件可能已变化），但把「与上一步相同」明确回给模型。
                        let duplicate = !self
                            .seen
                            .insert(format!("{}:{}", call.name, call.arguments));
                        progress |= !duplicate;
                        pause |= outcome.pause;
                        if let Some(prepared) = outcome.generation {
                            // 进入拆卡模式：下一轮请求起工具集切换，回合不中断。
                            self.enter_generation(prepared)?;
                            progress = true;
                        }
                        if let Some(message) = outcome.message {
                            if message.kind == "change" {
                                if let Some(pending) = self
                                    .session
                                    .messages
                                    .iter_mut()
                                    .find(|m| m.data["changeId"] == operation)
                                {
                                    pending.data = message.data;
                                }
                            } else {
                                self.session.messages.push(message);
                            }
                        }
                        // 画面随历史回给模型；跨回合重读时已不再需要。
                        let images = outcome.images;
                        if duplicate {
                            (
                                json!({"ok":true,"result":outcome.value,"duplicate":true,"hint":"这次调用与上一步完全相同，请换一个动作或更新计划"}),
                                images,
                            )
                        } else {
                            (json!({"ok":true,"result":outcome.value}), images)
                        }
                    }
                    Err(error) => {
                        if let Some(pending) = self
                            .session
                            .messages
                            .iter_mut()
                            .find(|m| m.data["changeId"] == operation)
                        {
                            pending.data["state"] = json!("failed");
                        }
                        self.session.messages.push(tools::block(
                            "status",
                            &error.message,
                            Value::Null,
                        ));
                        (json!({"ok":false,"error":error}), Vec::new())
                    }
                };
                // 会话记录只留文本与张数，Base64 图片不进入持久化文件。
                let mut record = tools::tool_message(&call.id, &value, &[]);
                if !images.is_empty() {
                    record["imageCount"] = json!(images.len());
                }
                exchange["results"][call_index] = record;
                self.session.messages[exchange_index].data = exchange.clone();
                self.history
                    .push(tools::tool_message(&call.id, &value, &images));
                repository.save_session(self.session)?;
            }
            // 生成结束：模型说明原因，或清单已全部落地（数量上限模式）。
            if generation_finish.is_some()
                || self
                    .generation
                    .as_ref()
                    .is_some_and(|mode| mode.prepared.session.fixed_complete())
            {
                self.finish_generation(generation_finish)?;
            }
            repository.save_session(self.session)?;
            if pause || self.waiting_user {
                self.seal_tree(false)?;
                self.waiting_user = true;
                self.goal_runtime.wait_for_user();
                autonomy::project(self.session, control, true);
                self.session.messages.retain(|m| m.kind != "running");
                self.session.completed_message_count = self.session.messages.len();
                repository.save_session(self.session)?;
                return Ok(TurnOutcome::Finish);
            }
            Ok(if progress {
                TurnOutcome::Progress
            } else {
                TurnOutcome::Stalled
            })
        })
    }

    /// 连续多轮无进展只提示值守者，是否停止由人决定。
    fn stalled(&mut self, step: u32, rounds: u32) {
        self.control.emit(AgentEvent::Stalled { step, rounds });
    }

    /// 用户取消不是工具失败：保留已完成的操作、文字与已通过校验的草稿。
    fn cancel(&mut self) -> Result<(), CommandError> {
        self.goal_runtime
            .stop(crate::services::agent_goal::StopReason::User);
        self.finish_generation(Some("已停止，保留已完成草稿".to_string()))?;
        Err(CommandError::new(
            "AGENT_CANCELLED",
            "已停止，已完成的操作保留",
        ))
    }
}
