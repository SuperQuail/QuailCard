use super::{
    generation_executor::execute_generation,
    generation_ports::{GenerationModel, PortFuture},
    AppServices, GenerationControl,
};
use crate::{
    ai::{validate_generation_input, GenerationSession, MultiToolRequest, ToolCallBatch},
    dictionary::Dictionary,
    error::CommandError,
    models::{GenerationInput, GenerationResult, ProviderConfig},
    storage::Storage,
};

/// 旧应用组合层适配器集中认证配置，不让生成执行器感知存储或网络实现。
struct ConfiguredGenerationModel<'a> {
    services: &'a AppServices,
    storage: &'a Storage,
    config: ProviderConfig,
}

impl GenerationModel for ConfiguredGenerationModel<'_> {
    /// 既有认证适配保持同一会话固定供应商配置。
    fn call<'a>(&'a self, request: MultiToolRequest<'a>) -> PortFuture<'a, ToolCallBatch> {
        Box::pin(
            self.services
                .call_configured_multi_tool(self.storage, &self.config, request),
        )
    }

    /// 增量通过同一认证适配透传，只有多工具请求需要。
    fn call_streaming<'a>(
        &'a self,
        request: MultiToolRequest<'a>,
        delta: &'a (dyn Fn(&str) + Send + Sync),
    ) -> PortFuture<'a, ToolCallBatch> {
        Box::pin(self.services.call_configured_multi_tool_streaming(
            self.storage,
            &self.config,
            request,
            Some(delta),
        ))
    }
}

impl AppServices {
    /// 兼容旧同步等待命令，并复用同一取消、预算和草稿执行器。
    pub async fn generate_cards(
        &self,
        storage: &Storage,
        dictionary: &Dictionary,
        input: GenerationInput,
    ) -> Result<GenerationResult, CommandError> {
        self.generate_cards_controlled(
            storage,
            dictionary,
            input,
            &GenerationControl::new(uuid::Uuid::now_v7().to_string()),
        )
        .await
    }

    /// 准备阶段也响应停止，启动失败保留可查询的安全终态；拆卡对话框与旧同步命令共用。
    pub(crate) async fn generate_cards_controlled(
        &self,
        storage: &Storage,
        dictionary: &Dictionary,
        input: GenerationInput,
        control: &GenerationControl,
    ) -> Result<GenerationResult, CommandError> {
        let config = storage.get_active_provider_config().await?;
        let mut input = input;
        input.source_text = input.source_text.replace("\r\n", "\n");
        let preparation = async {
            validate_generation_input(&input)?;
            let snapshot = storage.validate_generation_context(&input)?;
            if !input.images.is_empty() && !config.supports_vision {
                return Err(CommandError::validation(
                    "当前供应商未启用图片输入，请更换模型或在模型设置中开启",
                ));
            }
            Ok((snapshot, config))
        };
        // 不设准备阶段总时限：准备本身是本地校验，取消是唯一提前出口。
        let (snapshot, config) = tokio::select! {
            biased;
            _ = control.cancelled() => return Ok(GenerationResult { cards: vec![], warnings: vec!["已停止生成".to_string()] }),
            result = preparation => result?,
        };
        let session = GenerationSession::prepared(&input, snapshot.note_content, &snapshot.cards);
        let model = ConfiguredGenerationModel {
            services: self,
            storage,
            config,
        };
        execute_generation(&model, dictionary, &input, session, control).await
    }
}
