use super::{
    bridge,
    llm::diagnostics::IDLE_TIMEOUT_SECS,
    llm::request::Credential,
    llm::runtime::{LlmRuntime, StreamSinks},
    ProviderProtocol, ToolDefinition,
};
use crate::{
    error::CommandError,
    models::{ProviderConfig, OPENAI_SUBSCRIPTION_PROVIDER_TYPE},
    services::agent_ports::{AgentFuture, AgentModel, AgentModelReply},
};
use serde_json::Value;
use zeroize::Zeroizing;

/// 请求生命周期持有清零凭据，并固定当前轮次供应商配置；调用统一走 LlmRuntime。
pub(crate) struct ConfiguredAgentModel {
    runtime: LlmRuntime,
    config: ProviderConfig,
    credential: Zeroizing<String>,
    account: Option<String>,
    /// 每段对话的稳定身份，供 OpenCode 关联同一会话的请求。
    session_id: String,
    auth_type: &'static str,
    responses: bool,
}

impl ConfiguredAgentModel {
    /// 协议与认证组合在构造时校验，运行期只经注册表分发。
    pub(crate) fn new(
        config: ProviderConfig,
        credential: String,
        account: Option<String>,
        session_id: String,
    ) -> Result<Self, CommandError> {
        let credential = Zeroizing::new(credential);
        let auth_type = match config.auth_type.as_deref() {
            Some("api_key") => "api_key",
            Some("openai_oauth") => "openai_oauth",
            _ => return Err(CommandError::validation("当前认证与协议尚不支持 Agent")),
        };
        let protocol = ProviderProtocol::parse(&config.protocol)?;
        if auth_type == "openai_oauth" {
            if protocol != ProviderProtocol::OpenAiCompatible {
                return Err(CommandError::validation("当前认证与协议尚不支持 Agent"));
            }
            if config.provider_type != OPENAI_SUBSCRIPTION_PROVIDER_TYPE {
                return Err(CommandError::new(
                    "PROVIDER_AUTH_MISMATCH",
                    "ChatGPT OAuth 只能用于 OpenAI 订阅供应商",
                ));
            }
        }
        let runtime = super::configured_runtime()?;
        let responses = auth_type == "openai_oauth";
        Ok(Self {
            runtime,
            config,
            credential,
            account,
            session_id,
            auth_type,
            responses,
        })
    }

    /// 子会话复用已解析配置，但绑定独立模型会话身份，凭据副本仍由 Zeroizing 持有。
    pub(crate) fn for_session(&self, session_id: &str) -> Result<Self, CommandError> {
        Self::new(
            self.config.clone(),
            self.credential.to_string(),
            self.account.clone(),
            session_id.into(),
        )
    }

    /// 请求级凭据；凭据只覆盖一次调用，不驻留运行时。
    fn request_credential(&self) -> Credential {
        if self.responses {
            Credential::OAuth {
                access_token: Zeroizing::new(self.credential.to_string()),
                account_id: self.account.clone(),
            }
        } else {
            Credential::ApiKey(zeroize::Zeroizing::new(self.credential.to_string()))
        }
    }
}

impl AgentModel for ConfiguredAgentModel {
    /// 纯聊天入口没有外部关联身份。
    fn call<'a>(
        &'a self,
        system: &'a str,
        messages: &'a [Value],
        tools: &'a [ToolDefinition],
        delta: &'a (dyn Fn(&str) + Send + Sync),
        reasoning: &'a (dyn Fn(&str) + Send + Sync),
    ) -> AgentFuture<'a, AgentModelReply> {
        self.call_traced(system, messages, tools, delta, reasoning, "")
    }

    /// 视频任务可传入规范 UUID 关联阶段日志，普通聊天自动生成请求身份。
    fn call_traced<'a>(
        &'a self,
        system: &'a str,
        messages: &'a [Value],
        tools: &'a [ToolDefinition],
        delta: &'a (dyn Fn(&str) + Send + Sync),
        reasoning: &'a (dyn Fn(&str) + Send + Sync),
        trace: &'a str,
    ) -> AgentFuture<'a, AgentModelReply> {
        Box::pin(async move {
            let route = bridge::route(
                &self.config,
                self.auth_type,
                // 输出上限按当前选中模型可配；未配置时回退默认值，避免长回答被截断。
                Some(self.config.max_output_tokens()),
                None,
                true,
                IDLE_TIMEOUT_SECS * 1000,
                &self.session_id,
            )?;
            let request = bridge::agent_request(system, messages, tools);
            let credential = self.request_credential();
            let turn = self
                .runtime
                .stream(
                    &route,
                    &request,
                    &credential,
                    trace,
                    "AgentModel",
                    StreamSinks {
                        text: Some(delta),
                        reasoning: Some(reasoning),
                    },
                )
                .await?;
            if !matches!(
                turn.finish,
                super::llm::vocabulary::FinishReason::Stop
                    | super::llm::vocabulary::FinishReason::ToolCalls
            ) {
                return Err(CommandError::new(
                    "AGENT_INCOMPLETE_RESPONSE",
                    "模型响应被截断或异常结束，已停止自动执行；可发送消息继续",
                ));
            }
            Ok(bridge::agent_reply(turn, self.responses))
        })
    }
}
