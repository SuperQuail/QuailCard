use serde::Deserialize;
use serde_json::{json, Value};

use super::ToolDefinition;
use crate::{
    error::CommandError,
    models::{AiEvaluationContext, AiEvaluationResult},
};

#[derive(Deserialize)]
struct RawEvaluationResponse {
    schema_version: u32,
    is_correct: bool,
    feedback: String,
    missing_points: Vec<String>,
    suggested_answer: String,
}

/// 构造只包含当前题目上下文的单轮判定提示词。
pub fn build_evaluation_prompt(
    context: &AiEvaluationContext,
    user_answer: &str,
) -> Result<(String, String), CommandError> {
    if user_answer.trim().is_empty() || user_answer.chars().count() > 8_000 {
        return Err(CommandError::validation("回答长度必须为 1-8,000 个字符"));
    }
    let payload = serde_json::json!({
        "question": context.question,
        "user_answer": user_answer,
        "reference_answer": context.reference_answer,
        "rubric_points": context.rubric_points,
    });
    let system = "你是严格但允许同义表达的学习答案判定器。只判断知识内容，不根据长度或文风判断。每次请求相互独立。必须调用 submit_evaluation 工具提交判定，禁止使用普通文本回答。".to_string();
    Ok((system, payload.to_string()))
}

/// 构造 AI 问答判定使用的严格工具 Schema。
pub fn evaluation_tool() -> ToolDefinition {
    ToolDefinition {
        name: "submit_evaluation",
        description: "提交当前一道问答题的语义判定和修改建议",
        input_schema: json!({
            "type": "object",
            "properties": {
                "schema_version": { "type": "integer", "enum": [1] },
                "is_correct": { "type": "boolean" },
                "feedback": { "type": "string" },
                "missing_points": {
                    "type": "array",
                    "items": { "type": "string" }
                },
                "suggested_answer": { "type": "string" }
            },
            "required": [
                "schema_version",
                "is_correct",
                "feedback",
                "missing_points",
                "suggested_answer"
            ],
            "additionalProperties": false
        }),
    }
}

/// 解析并校验模型工具返回的单题判定参数。
pub fn parse_evaluation_response(arguments: Value) -> Result<AiEvaluationResult, CommandError> {
    let raw: RawEvaluationResponse = serde_json::from_value(arguments).map_err(|_| {
        CommandError::provider(
            "PROVIDER_TOOL_RESPONSE_INVALID",
            "判定工具参数不符合约定结构",
        )
    })?;
    if raw.schema_version != 1 || raw.feedback.trim().is_empty() {
        return Err(CommandError::provider(
            "PROVIDER_RESPONSE_INVALID",
            "模型返回的判定字段不完整",
        ));
    }
    Ok(AiEvaluationResult {
        is_correct: raw.is_correct,
        feedback: raw.feedback,
        missing_points: raw.missing_points,
        suggested_answer: raw.suggested_answer,
        progress: None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    /// 判定解析器保留布尔结果和修改建议。
    fn parses_evaluation_result() {
        let result = parse_evaluation_response(json!({
            "schema_version": 1,
            "is_correct": false,
            "feedback": "缺少关键关系",
            "missing_points": ["所有权"],
            "suggested_answer": "补充所有权关系"
        }))
        .expect("解析判定失败");
        assert!(!result.is_correct);
        assert_eq!(result.missing_points, ["所有权"]);
    }

    #[test]
    /// 判定工具禁止模型添加未声明字段。
    fn builds_strict_evaluation_schema() {
        let tool = evaluation_tool();
        assert_eq!(tool.name, "submit_evaluation");
        assert_eq!(tool.input_schema["additionalProperties"], false);
        assert_eq!(
            tool.input_schema["properties"]["is_correct"]["type"],
            "boolean"
        );
    }
}
