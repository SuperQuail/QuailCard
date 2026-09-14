//! 组合根为每个子会话装配拥有型依赖，领域管理器不接触 Tauri 或网络实现。
use crate::{
    agent_execution_settings::AgentExecutionSettings,
    agent_models::{AgentInput, AgentSession},
    ai::agent::ConfiguredAgentModel,
    dictionary::Dictionary,
    error::CommandError,
    services::{
        agent,
        agent_ports::AgentPorts,
        agent_tasks::AgentControl,
        subagents::{ChildExecution, ChildExecutor, ChildFuture, SubagentLimits, Subagents},
    },
    storage::agent::AgentFiles,
};
use futures_util::FutureExt;
use std::{path::PathBuf, sync::Arc};
use tauri::Manager;

struct Executor {
    app: tauri::AppHandle,
    root: PathBuf,
    model: Arc<ConfiguredAgentModel>,
    config: crate::models::ProviderConfig,
    provider_id: String,
    settings: AgentExecutionSettings,
}

impl ChildExecutor for Executor {
    /// 子任务固定库、模型配置与自己的 session identity；返回前等待 runner 持久化。
    fn execute(
        &self,
        request: ChildExecution,
        tree: Arc<Subagents>,
        control: Arc<AgentControl>,
    ) -> ChildFuture {
        let app = self.app.clone();
        let root = self.root.clone();
        let model = self.model.clone();
        let config = self.config.clone();
        let provider_id = self.provider_id.clone();
        let settings = self.settings.clone();
        Box::pin(async move {
            let mut session = request.session;
            let previous = session.messages.len();
            let repository = AgentFiles::new(&root)?;
            let model = model.for_session(&session.id)?;
            let input = AgentInput {
                session_id: session.id.clone(),
                request_id: request.execution_id,
                content: format!(
                    "委派任务：{}\n消息身份：{}\n{}",
                    request.description, request.message_id, request.prompt
                ),
                provider_id: provider_id.clone(),
                selected_paths: session.selected_paths.clone(),
                images: vec![],
            };
            let dictionary = app.state::<Dictionary>();
            let learning = crate::agent_learning::LearningAdapter {
                app: app.clone(),
                config,
            };
            let video = crate::agent_video::VideoAdapter {
                app: app.clone(),
                provider_id,
                owner: format!("agent:{}", session.id),
            };
            let cards = crate::agent_cards::CardsAdapter { app: app.clone() };
            agent::execute_with(
                AgentPorts {
                    model: &model,
                    repository: &repository,
                    learning: &learning,
                    video: &video,
                    dictionary: &*dictionary,
                    cards: &cards,
                },
                &mut session,
                &input,
                &control,
                Some(tree),
                settings,
            )
            .await?;
            if control.is_cancelled() {
                return Err(CommandError::new("AGENT_CANCELLED", "子 Agent 已停止"));
            }
            Ok(session
                .messages
                .iter()
                .skip(previous)
                .rev()
                .find(|m| m.role == "assistant" && m.kind == "text")
                .map(|m| m.content.clone())
                .unwrap_or_else(|| "子 Agent 未留下结果，请追加明确任务".into()))
        })
    }
}

/// 根控制器仍处于 busy 时停止并收取所有后代，防止切库后子任务访问新的 Storage。
pub(crate) async fn execute_root(
    app: &tauri::AppHandle,
    model: ConfiguredAgentModel,
    learning: crate::agent_learning::LearningAdapter,
    session: &mut AgentSession,
    input: &AgentInput,
    control: Arc<AgentControl>,
    settings: AgentExecutionSettings,
) -> Result<(), CommandError> {
    let repository = Arc::new(AgentFiles::new(std::path::Path::new(&control.root))?);
    let model = Arc::new(model);
    let executor = Arc::new(Executor {
        app: app.clone(),
        root: PathBuf::from(&control.root),
        model: model.clone(),
        config: learning.config.clone(),
        provider_id: input.provider_id.clone(),
        settings: settings.clone(),
    });
    let limits = SubagentLimits {
        max_depth: settings.max_depth,
        max_agents: settings.max_agents,
        max_concurrent_models: settings.max_concurrent_models,
        ..Default::default()
    };
    let tree = Subagents::new(
        session.clone(),
        control.clone(),
        repository.clone(),
        executor,
        limits,
    );
    control.attach_subagents(&tree);
    let dictionary = app.state::<Dictionary>();
    let video = crate::agent_video::VideoAdapter {
        app: app.clone(),
        provider_id: input.provider_id.clone(),
        owner: format!("agent:{}", session.id),
    };
    let cards = crate::agent_cards::CardsAdapter { app: app.clone() };
    let result = std::panic::AssertUnwindSafe(agent::execute_with(
        AgentPorts {
            model: &*model,
            repository: &*repository,
            learning: &learning,
            video: &video,
            dictionary: &*dictionary,
            cards: &cards,
        },
        session,
        input,
        &control,
        Some(tree.clone()),
        settings,
    ))
    .catch_unwind()
    .await
    .unwrap_or_else(|_| {
        Err(CommandError::new(
            "AGENT_EXECUTION_FAILED",
            "Agent 执行异常，已停止所有子任务",
        ))
    });
    tree.shutdown().await;
    result
}
