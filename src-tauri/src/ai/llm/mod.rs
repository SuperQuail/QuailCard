//! 供应商中立的模型调用层。
//!
//! 词汇、chunk 协议、assembler、路由与 adapter 注册表；Agent、生成、判定、
//! 连接测试都经 LlmRuntime 调用。本目录禁止依赖
//! services / storage / tauri / dictionary / vaultfs，由 architecture_tests 门禁。

pub(crate) mod adapter;
pub(crate) mod assembler;
pub(crate) mod diagnostics;
mod echo;
pub(crate) mod identity;
pub(crate) mod runtime;
pub(crate) mod sse;

// 协议词汇与失败码是完整分类；部分变体要等新协议接线后才会被构造。
#[allow(dead_code)]
pub(crate) mod chunk;
#[allow(dead_code)]
pub(crate) mod failure;
#[allow(dead_code)]
pub(crate) mod request;
#[allow(dead_code)]
pub(crate) mod vocabulary;
