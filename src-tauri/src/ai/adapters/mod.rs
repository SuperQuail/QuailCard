//! 供应商 wire 适配器：每个协议一个文件，只依赖 ai::llm 的中立词汇。

#[path = "anthropic.rs"]
pub(crate) mod anthropic;
#[path = "chat.rs"]
pub(crate) mod chat;
#[path = "responses.rs"]
pub(crate) mod responses;

#[cfg(test)]
#[path = "contract_tests.rs"]
mod contract_tests;

use std::sync::Arc;

use crate::ai::llm::adapter::{route_key, AdapterRegistry};
use crate::error::CommandError;

/// 把内置协议注册进运行时；新增协议只需在这里加一行。
pub(crate) fn register_builtin(registry: &AdapterRegistry) -> Result<(), CommandError> {
    registry.register(
        &[route_key("api_key", "OpenAI Compatible")],
        Arc::new(chat::Chat),
    )?;
    registry.register(
        &[route_key("api_key", "Anthropic Messages")],
        Arc::new(anthropic::Anthropic),
    )?;
    registry.register(
        &[route_key("openai_oauth", "OpenAI Compatible")],
        Arc::new(responses::Responses),
    )
}
