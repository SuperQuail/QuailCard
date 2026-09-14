//! 工具参数的前置校验：对象根、封闭 Schema 的未知字段与必填字段。

use serde_json::Value;

use super::spec::ToolSpec;
use crate::error::CommandError;

/// pre 校验：对象根、封闭 Schema 的未知字段、必填字段存在。
pub(crate) fn validate_arguments(spec: &ToolSpec, arguments: &Value) -> Result<(), CommandError> {
    let object = arguments
        .as_object()
        .ok_or_else(|| CommandError::validation("工具参数必须为完整 JSON 对象"))?;
    let properties = spec
        .schema
        .get("properties")
        .and_then(Value::as_object)
        .ok_or_else(|| CommandError::validation("工具 Schema 缺少属性定义"))?;
    let closed = spec
        .schema
        .get("additionalProperties")
        .and_then(Value::as_bool)
        == Some(false);
    if closed && object.keys().any(|key| !properties.contains_key(key)) {
        return Err(CommandError::validation("工具参数字段不符合定义"));
    }
    if let Some(required) = spec.schema.get("required").and_then(Value::as_array) {
        if required
            .iter()
            .filter_map(Value::as_str)
            .any(|key| !object.contains_key(key))
        {
            return Err(CommandError::validation("工具参数字段不符合定义"));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ai::tools::spec::ToolEffect;
    use serde_json::json;

    fn spec() -> ToolSpec {
        ToolSpec {
            name: "emit",
            description: "d",
            schema: json!({
                "type": "object",
                "properties": {"a": {"type": "string"}},
                "required": ["a"],
                "additionalProperties": false
            }),
            effect: ToolEffect::Read,
        }
    }

    #[test]
    /// 非对象、未知字段与缺必填都被拒绝。
    fn rejects_structural_violations() {
        assert!(validate_arguments(&spec(), &json!("text")).is_err());
        assert!(validate_arguments(&spec(), &json!({"b": 1})).is_err());
        assert!(validate_arguments(&spec(), &json!({"a": "x"})).is_ok());
    }
}
