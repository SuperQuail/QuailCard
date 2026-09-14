use super::*;
use crate::ai::{
    generation_mode_prompt, generation_tools, ToolArguments, ToolCallBatch, ToolCallResult,
    ToolMessage,
};
use crate::services::{
    generation_ports::DictionaryLookup,
    generation_round::{self, process_generation_round},
    GenerationControl,
};

/// 拆卡模式的可变状态：草稿会话、生成工具表与词典去重集合都由 Agent 循环持有。
pub(super) struct GenerationMode {
    pub(super) prepared: PreparedGeneration,
    pub(super) tools: generation_round::GenerationTools,
    lookups: std::collections::HashSet<String>,
    pub(super) control: GenerationControl,
    system_length: usize,
}

impl AgentTurn<'_> {
    /// 生成模式下模型可见的工具集 = Agent 工具 + 生成工具。
    pub(super) fn tool_definitions(&self) -> Vec<ToolDefinition> {
        if self.terminal_goal || self.waiting_user {
            return Vec::new();
        }
        let mut definitions = self.definitions.clone();
        definitions.extend(
            self.runtime_tools
                .iter()
                .filter(|tool| {
                    self.authority != Authority::Child
                        || !["create_goal", "update_goal", "wait_for_user"]
                            .contains(&tool.spec.name)
                })
                .map(|tool| tool.spec.definition()),
        );
        if let Some(mode) = &self.generation {
            definitions.extend(mode.tools.definitions());
        }
        definitions
    }

    /// 安装拆卡模式：系统提示给出两阶段流程，下一次请求起工具集切换。
    pub(super) fn enter_generation(
        &mut self,
        prepared: PreparedGeneration,
    ) -> Result<(), CommandError> {
        if self.generation.is_some() {
            return Err(CommandError::new(
                "GENERATION_ACTIVE",
                "请先完成当前拆卡任务，不要重新开始",
            ));
        }
        let prompt = generation_mode_prompt(&prepared.input)?;
        let tools = generation_round::GenerationTools::build(generation_tools(&prepared.input)?)?;
        let system_length = self.system.len();
        self.system = format!("{}\n{prompt}", self.system);
        self.generation = Some(GenerationMode {
            prepared,
            tools,
            lookups: std::collections::HashSet::new(),
            control: GenerationControl::new(uuid::Uuid::now_v7().to_string()),
            system_length,
        });
        Ok(())
    }

    /// 结束拆卡模式：草稿落成可采纳消息；零草稿时只留下警告，界面不出现空草稿块。
    pub(super) fn finish_generation(&mut self, reason: Option<String>) -> Result<(), CommandError> {
        let Some(mode) = self.generation.take() else {
            return Ok(());
        };
        self.system.truncate(mode.system_length);
        let prepared = mode.prepared;
        let result = prepared.session.finish(reason);
        if !result.cards.is_empty() {
            self.seal_tree(false)?;
            self.waiting_user = true;
            self.goal_runtime.wait_for_user();
            let data = json!({
                "path": prepared.path,
                "kind": prepared.kind,
                "expectedVaultPath": prepared.expected_vault_path,
                "expectedNoteHash": prepared.expected_note_hash,
                "cards": result.cards,
                "adoptedIds": [],
                "goalId": self.session.goal.as_ref().map(|goal| &goal.id),
                "goalRevision": self.session.goal.as_ref().map(|goal| goal.revision),
                "adoptionResolved": false,
            });
            self.session
                .messages
                .push(tools::block("drafts", "生成卡片草稿", data));
        }
        if !result.warnings.is_empty() {
            self.session.messages.push(tools::block(
                "status",
                &result.warnings.join("；"),
                Value::Null,
            ));
        }
        self.ports.repository.save_session(self.session)
    }
}

/// 同一条响应里的生成调用按生成管线整批执行，结果按 call id 返回。
///
/// 整批执行才能保住 plan→lookup→emit→finish 的顺序与「同轮查词则推迟落卡」的规则；
/// 取消时给每个调用一个合成结果，循环仍能写出配对历史。
pub(super) async fn run_generation_batch(
    dictionary: &dyn DictionaryLookup,
    mode: &mut GenerationMode,
    calls: &[AgentCall],
    control: &AgentControl,
) -> (
    std::collections::HashMap<String, String>,
    bool,
    Option<String>,
) {
    let cancelled =
        "{\"ok\":false,\"error\":{\"code\":\"AGENT_CANCELLED\",\"message\":\"已停止\"}}"
            .to_string();
    let batch = ToolCallBatch {
        calls: calls
            .iter()
            .map(|call| ToolCallResult {
                id: call.id.clone(),
                item_id: None,
                name: call.name.clone(),
                arguments: ToolArguments::Valid(call.arguments.clone()),
            })
            .collect(),
        continuation_items: Vec::new(),
    };
    let round = tokio::select! { biased;
        _ = control.cancelled() => None,
        round = process_generation_round(
            dictionary,
            &mode.prepared.input,
            &mut mode.prepared.session,
            batch,
            &mode.tools,
            &mut mode.lookups,
            &mode.control,
        ) => Some(round),
    };
    let Some(round) = round else {
        return (
            calls
                .iter()
                .map(|call| (call.id.clone(), cancelled.clone()))
                .collect(),
            false,
            None,
        );
    };
    let mut contents = std::collections::HashMap::new();
    for message in round.history {
        if let ToolMessage::ToolResult { id, content } = message {
            contents.insert(id, content);
        }
    }
    // 未产生结果的调用也要有回执，避免模型看到缺失的配对。
    for call in calls {
        contents
            .entry(call.id.clone())
            .or_insert_with(|| cancelled.clone());
    }
    (contents, round.progressed, round.finish_reason)
}
