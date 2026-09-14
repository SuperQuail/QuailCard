//! 统一工具层：供应商中立的工具声明与前置换校验。
//!
//! Agent、卡片生成与 AI 判定共用同一份 ToolSpec 与参数校验；执行上下文与结果投影
//! 由各 scope 自己持有，注册/执行不合流的原因见 docs/agent-refactor.md §14。

pub(crate) mod spec;
pub(crate) mod validate;
