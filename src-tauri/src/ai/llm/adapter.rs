//! adapter 端口与路由注册表；新增协议只新增 adapter 与一次 register。

use std::collections::HashMap;
use std::sync::{Arc, RwLock};

use reqwest::Request;
use serde_json::Value;

use super::chunk::StreamChunk;
use super::request::{Credential, ModelRequest, ModelRoute};
use crate::error::CommandError;

/// 供应商 wire 翻译端口。
pub(crate) trait LlmAdapter: Send + Sync {
    /// 固定协议标签，进入日志与诊断，不能是用户配置字符串。
    fn protocol(&self) -> &'static str;

    /// 构造 wire 请求；凭据按请求传入，不驻留 adapter。
    fn build(
        &self,
        client: &reqwest::Client,
        route: &ModelRoute,
        request: &ModelRequest,
        credential: &Credential,
    ) -> Result<Request, CommandError>;

    /// 把一个 SSE data 事件翻译成零到多个 chunk；不认识的返回空。
    fn translate(&self, event: &Value) -> Vec<StreamChunk>;

    /// 非流式 JSON 体的等价翻译；默认协议不支持。
    fn translate_json(&self, _body: &Value) -> Vec<StreamChunk> {
        Vec::new()
    }

    /// 该事件是否表示流正常收束；没有 [DONE] 的协议用它判定完整结束。
    fn is_terminal(&self, _event: &Value) -> bool {
        false
    }
}

/// 路由键：认证方式与协议共同决定 adapter。
pub(crate) fn route_key(auth_type: &str, protocol: &str) -> String {
    format!("{auth_type}|{protocol}")
}

/// 路由到 adapter 的全量映射；注册全有或全无，替换是原子的。
#[derive(Default)]
pub(crate) struct AdapterRegistry {
    routes: RwLock<HashMap<String, Arc<dyn LlmAdapter>>>,
}

impl AdapterRegistry {
    /// 注册一组路由；任一冲突即整体拒绝，避免半个注册生效。
    pub(crate) fn register(
        &self,
        keys: &[String],
        adapter: Arc<dyn LlmAdapter>,
    ) -> Result<(), CommandError> {
        let mut routes = self
            .routes
            .write()
            .unwrap_or_else(|poison| poison.into_inner());
        if keys.iter().any(|key| routes.contains_key(key)) {
            return Err(CommandError::provider(
                "DUPLICATE_ADAPTER",
                "同一模型路由被重复注册",
            ));
        }
        for key in keys {
            routes.insert(key.clone(), adapter.clone());
        }
        Ok(())
    }

    /// 解析唯一 adapter；未命中返回 NO_ADAPTER。
    pub(crate) fn resolve(&self, key: &str) -> Result<Arc<dyn LlmAdapter>, CommandError> {
        self.routes
            .read()
            .unwrap_or_else(|poison| poison.into_inner())
            .get(key)
            .cloned()
            .ok_or_else(|| CommandError::provider("NO_ADAPTER", "当前供应商协议尚未注册"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 最小 adapter，只证明路由解析。
    struct Fake(&'static str);

    impl LlmAdapter for Fake {
        fn protocol(&self) -> &'static str {
            self.0
        }

        fn build(
            &self,
            _client: &reqwest::Client,
            _route: &ModelRoute,
            _request: &ModelRequest,
            _credential: &Credential,
        ) -> Result<Request, CommandError> {
            Err(CommandError::provider("NO_ADAPTER", "测试不构造请求"))
        }

        fn translate(&self, _event: &Value) -> Vec<StreamChunk> {
            Vec::new()
        }
    }

    #[test]
    /// 路由键由认证与协议组合，重复注册被整体拒绝。
    fn registers_routes_without_partial_state() {
        let registry = AdapterRegistry::default();
        let chat = route_key("api_key", "OpenAI Compatible");
        let anthropic = route_key("api_key", "Anthropic Messages");
        registry
            .register(std::slice::from_ref(&chat), Arc::new(Fake("openai_chat")))
            .unwrap();
        assert_eq!(registry.resolve(&chat).unwrap().protocol(), "openai_chat");

        let duplicate = registry.register(
            &[anthropic.clone(), chat.clone()],
            Arc::new(Fake("anthropic_messages")),
        );
        assert_eq!(duplicate.unwrap_err().code, "DUPLICATE_ADAPTER");
        assert!(registry.resolve(&anthropic).is_err());
    }

    #[test]
    /// 未注册路由返回 NO_ADAPTER，不静默回退到其他协议。
    fn unknown_route_is_explicit() {
        let registry = AdapterRegistry::default();
        let error = match registry.resolve("api_key|OpenAI Compatible") {
            Err(error) => error,
            Ok(_) => panic!("未注册路由必须失败"),
        };
        assert_eq!(error.code, "NO_ADAPTER");
    }
}
