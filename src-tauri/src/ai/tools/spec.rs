//! 工具声明与结果的中立类型。
//!
//! Agent、卡片生成与 AI 判定共用同一份 ToolSpec；执行上下文由各 scope 自己持有。

use serde_json::{json, Value};

/// 工具副作用分类，决定写前握手与后续并发调度。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ToolEffect {
    Read,
    /// 直接改写笔记文件：需要编辑器写前握手与 change 记录。
    Write,
    /// 修改派生数据（如删除卡片）：会改变状态，但不触碰正在编辑的笔记正文。
    Mutate,
    External,
}

/// 模型可见的工具声明。
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct ToolSpec {
    pub name: &'static str,
    pub description: &'static str,
    pub schema: Value,
    pub effect: ToolEffect,
}

impl ToolSpec {
    /// 转成模型请求使用的线格式；副作用不进入请求。
    pub(crate) fn definition(&self) -> crate::ai::ToolDefinition {
        crate::ai::ToolDefinition {
            name: self.name,
            description: self.description,
            input_schema: self.schema.clone(),
        }
    }

    /// 是否改写正在编辑的笔记正文，决定是否需要编辑器写前握手；
    /// Mutate（如删除卡片）会改变状态但不属于这一类。
    pub(crate) fn writes(&self) -> bool {
        self.effect == ToolEffect::Write
    }
}

/// 一次工具调用的安全结果；失败不携带原始载荷。
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct ToolResult {
    pub value: Value,
    pub is_error: bool,
}

impl ToolResult {
    /// 失败结果，只保留稳定 code 与安全文案。
    pub(crate) fn error(code: &str, message: &str) -> Self {
        Self {
            value: json!({"ok": false, "error": {"code": code, "message": message}}),
            is_error: true,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    /// 线格式只包含 name/description/schema，副作用留在本层。
    fn definition_drops_execution_fields() {
        let spec = ToolSpec {
            name: "emit",
            description: "d",
            schema: json!({"type": "object"}),
            effect: ToolEffect::Write,
        };
        let definition = spec.definition();
        assert_eq!(definition.name, "emit");
        assert_eq!(definition.input_schema["type"], "object");
        assert!(spec.writes());
    }

    #[test]
    /// 失败结果只包含 code 与安全文案。
    fn error_result_is_safe() {
        let result = ToolResult::error("INVALID_JSON", "参数不是有效 JSON");
        assert!(result.is_error);
        assert_eq!(result.value["error"]["code"], "INVALID_JSON");
    }
}
