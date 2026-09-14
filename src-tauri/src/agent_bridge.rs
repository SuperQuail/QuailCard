//! 应用组合根：把具体存储、认证与 Agent 端口连接起来。
use crate::{
    agent_models::{AgentChange, AgentInput, AgentRun},
    ai::agent::ConfiguredAgentModel,
    error::CommandError,
    models::ProviderConfig,
    services::{
        agent,
        agent_ports::{AgentFuture, AgentRepository},
        agent_tasks::AgentTasks,
        AppServices,
    },
    storage::{agent::AgentFiles, Storage},
    vaultfs::VaultState,
};
use tauri::Manager;

/// 从当前知识库捕获固定根目录，任务执行期间不再跟随界面切换。
pub(crate) fn files(app: &tauri::AppHandle) -> Result<AgentFiles, CommandError> {
    let root = app
        .state::<VaultState>()
        .root()?
        .ok_or_else(|| CommandError::new("VAULT_NOT_OPEN", "请先打开知识库"))?;
    AgentFiles::new(&root)
}

/// 任务停止后才允许移除历史，避免后台轮询继续更新被删除会话。
pub(crate) fn delete_session(app: &tauri::AppHandle, id: &str) -> Result<(), CommandError> {
    app.state::<AgentTasks>().ensure_idle()?;
    files(app)?.delete_session(id)
}

type CredentialLoader = for<'a> fn(
    &'a AppServices,
    &'a Storage,
    &'a ProviderConfig,
) -> AgentFuture<'a, (String, Option<String>)>;

/// API Key 凭据仅在配置模型时短暂使用，随后由模型的 Zeroizing 持有。
fn api_key<'a>(
    services: &'a AppServices,
    storage: &'a Storage,
    config: &'a ProviderConfig,
) -> AgentFuture<'a, (String, Option<String>)> {
    Box::pin(async move { Ok((services.load_api_key(storage, config).await?, None)) })
}
/// OAuth 沿用现有刷新机制，不复制登录业务规则。
fn oauth<'a>(
    services: &'a AppServices,
    storage: &'a Storage,
    config: &'a ProviderConfig,
) -> AgentFuture<'a, (String, Option<String>)> {
    Box::pin(async move {
        let mut access = services.load_openai_access(storage, config).await?;
        Ok((
            std::mem::take(&mut access.access_token),
            access.account_id.take(),
        ))
    })
}

/// 凭据加载通过注册表扩展，不在 Agent 用例中分支判断供应商；
/// 会话 ID 是对话身份，由适配器用于 OpenCode 的会话请求头。
pub(crate) async fn model(
    app: &tauri::AppHandle,
    provider: &str,
    session_id: &str,
) -> Result<(ConfiguredAgentModel, crate::agent_learning::LearningAdapter), CommandError> {
    let services = app.state::<AppServices>();
    let storage = app.state::<Storage>();
    let config = storage
        .get_provider_config(provider)
        .await?
        .ok_or_else(|| CommandError::validation("请选择已配置的模型"))?;
    let loaders: &[(&str, CredentialLoader)] = &[("api_key", api_key), ("openai_oauth", oauth)];
    let loader = loaders
        .iter()
        .find(|(name, _)| Some(*name) == config.auth_type.as_deref())
        .ok_or_else(|| {
            CommandError::new(
                "PROVIDER_CREDENTIAL_MISSING",
                "请先配置 API Key 或登录 ChatGPT",
            )
        })?
        .1;
    let (secret, account) = loader(&services, &storage, &config).await?;
    let learning = crate::agent_learning::LearningAdapter {
        app: app.clone(),
        config: config.clone(),
    };
    Ok((
        ConfiguredAgentModel::new(config, secret, account, session_id.to_string())?,
        learning,
    ))
}

/// 先登记任务再派发后台执行，取消和快速重试不会产生重复写入。
pub(crate) async fn start(
    app: tauri::AppHandle,
    owner: String,
    input: AgentInput,
) -> Result<AgentRun, CommandError> {
    uuid::Uuid::parse_str(&input.request_id)
        .map_err(|_| CommandError::validation("请求 ID 无效"))?;
    input.validate_images()?;
    if (input.content.trim().is_empty() && input.images.is_empty())
        || input.content.chars().count() > 16_000
        || input.selected_paths.len() > 50
    {
        return Err(CommandError::validation(
            "请提供文字或图片，文字最多 16000 字，指定笔记最多 50 篇",
        ));
    }
    // 先完成异步配置读取，再同步捕获并登记固定知识库，避免 await 期间切库。
    let settings = app.state::<Storage>().agent_execution_settings().await?;
    let repository = files(&app)?;
    let mut session = repository.session(&input.session_id)?;
    if session.parent_session_id.is_some() {
        return Err(CommandError::validation(
            "子 Agent 只能由父会话继续，不能作为根用户请求启动",
        ));
    }
    if session
        .messages
        .iter()
        .any(|m| m.data["requestId"] == input.request_id)
    {
        if let Ok(control) = app.state::<AgentTasks>().get(&owner, &input.request_id) {
            return Ok(control.snapshot());
        }
        return Err(CommandError::new(
            "AGENT_ALREADY_HANDLED",
            "此消息已处理，请查看会话历史",
        ));
    }
    for path in &input.selected_paths {
        repository.read(path)?;
    }
    let root = app
        .state::<VaultState>()
        .root()?
        .unwrap()
        .to_string_lossy()
        .to_string();
    let (control, fresh) =
        app.state::<AgentTasks>()
            .register(&owner, &root, &input.request_id, &input.session_id)?;
    if fresh {
        if let Err(error) = agent::begin(&repository, &mut session, &input) {
            control.complete(Some(&error));
            return Err(error);
        }
        let task_control = control.clone();
        tauri::async_runtime::spawn(async move {
            // 认证阶段允许直接取消；执行阶段必须协作收尾而非丢弃 future。
            let configured = tokio::select! { biased;
                _ = task_control.cancelled() => Err(CommandError::new("AGENT_CANCELLED", "已停止")),
                result = model(&app, &input.provider_id, &session.id) => result,
            };
            let result = match configured {
                Ok((model, learning)) => {
                    crate::agent_child_executor::execute_root(
                        &app,
                        model,
                        learning,
                        &mut session,
                        &input,
                        task_control.clone(),
                        settings,
                    )
                    .await
                }
                Err(error) => Err(error),
            };
            let mut result = result;
            if session
                .messages
                .iter()
                .any(|message| message.kind == "running")
            {
                session.messages.retain(|message| message.kind != "running");
                if let Err(error) = &result {
                    session.messages.push(crate::agent_models::AgentMessage {
                        id: uuid::Uuid::now_v7().to_string(),
                        role: "assistant".into(),
                        kind: "status".into(),
                        content: error.message.clone(),
                        data: serde_json::Value::Null,
                    });
                }
                if let Err(error) = repository.save_session(&session) {
                    result = Err(error);
                }
            }
            task_control.complete(result.as_ref().err());
        });
    }
    Ok(control.snapshot())
}

/// 用户只提交选中草稿身份；内容和来源由后端保存的生成结果恢复。
pub(crate) async fn adopt(
    app: &tauri::AppHandle,
    session_id: &str,
    message_id: &str,
    ids: &[String],
) -> Result<crate::agent_models::AgentSession, CommandError> {
    app.state::<AgentTasks>().ensure_idle()?;
    let repository = files(app)?;
    let mut session = repository.session(session_id)?;
    let message = session
        .messages
        .iter_mut()
        .find(|m| m.id == message_id && m.kind == "drafts")
        .ok_or_else(|| CommandError::validation("卡片草稿消息不存在"))?;
    let cards: Vec<crate::models::GeneratedCard> =
        serde_json::from_value(message.data["cards"].clone())
            .map_err(|_| CommandError::validation("卡片草稿记录无效"))?;
    if ids.is_empty() {
        return Err(CommandError::validation("请选择要采纳的卡片草稿"));
    }
    if ids
        .iter()
        .any(|id| !cards.iter().any(|card| &card.draft_id == id))
    {
        return Err(CommandError::validation(
            "所选草稿不属于本次生成，请重新选择",
        ));
    }
    let input = crate::models::AdoptCardsInput {
        expected_vault_path: message.data["expectedVaultPath"]
            .as_str()
            .unwrap_or("")
            .into(),
        expected_note_hash: message.data["expectedNoteHash"]
            .as_str()
            .unwrap_or("")
            .into(),
        note_path: message.data["path"].as_str().unwrap_or("").into(),
        kind: message.data["kind"].as_str().unwrap_or("").into(),
        cards: cards
            .into_iter()
            .filter(|card| ids.contains(&card.draft_id))
            .collect(),
    };
    let result = app.state::<Storage>().adopt_cards(&input).await?;
    let mut adopted = message.data["adoptedIds"]
        .as_array()
        .cloned()
        .unwrap_or_default();
    for id in result
        .added_ids
        .iter()
        .chain(&result.existing_ids)
        .chain(&result.duplicate_ids)
    {
        let id = serde_json::Value::String(id.clone());
        if !adopted.contains(&id) {
            adopted.push(id);
        }
    }
    message.data["adoptedIds"] = serde_json::Value::Array(adopted);
    // 用户采纳所选集合是本批明确决定，不会自动保存未选择草稿。
    message.data["adoptionResolved"] = serde_json::Value::Bool(true);
    repository.save_session(&session)?;
    repository.public_session(session_id)
}

/// 撤销先检查真实卡片关联，文件恢复后刷新索引。
pub(crate) async fn undo(app: &tauri::AppHandle, id: &str) -> Result<AgentChange, CommandError> {
    app.state::<AgentTasks>().ensure_idle()?;
    let repository = files(app)?;
    let change = repository.get_change(id)?;
    let storage = app.state::<Storage>();
    let has_cards = !storage.list_note_cards(&change.path).await?.is_empty();
    let result = repository.undo(id, has_cards)?;
    let vault = app.state::<VaultState>();
    storage
        .rescan_vault(&vault.root()?.unwrap(), &vault.scan()?)
        .await?;
    Ok(result)
}
