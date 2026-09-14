//! 视频工具专属参数定义与校验，不扩大普通笔记工具的输入契约。
use super::handlers::string;
use super::{block, AgentFuture, ToolContext, ToolImage, ToolOutcome};
use crate::error::CommandError;
use serde_json::{json, Value};

/// 未填写的选项采用当前设置，空分 P 采用 URL 的 p 参数或 P1。
pub(super) fn video_schema() -> Value {
    json!({"url":{"type":"string"},
        "pages":{"type":"array","items":{"type":"integer","minimum":1},"maxItems":50},
        "quality":{"type":"integer","minimum":1,"maximum":127},
        "screenshots":{"type":"boolean"},"forceTranscribe":{"type":"boolean"}})
}

/// 真实执行再检查整数范围，不能依靠模型遵守 JSON Schema。
pub(super) fn validate_video_args(args: &Value) -> Result<(), CommandError> {
    for (key, value) in args
        .as_object()
        .ok_or_else(|| CommandError::validation("视频参数必须为对象"))?
    {
        let valid = match key.as_str() {
            "url" | "taskId" => value
                .as_str()
                .is_some_and(|s| !s.is_empty() && s.len() <= 4096),
            "pages" => value.as_array().is_some_and(|pages| {
                pages.len() <= 50
                    && pages
                        .iter()
                        .all(|p| p.as_u64().is_some_and(|n| n > 0 && n <= u32::MAX as u64))
            }),
            "quality" => value.as_u64().is_some_and(|n| (1..=127).contains(&n)),
            // 画面秒数允许小数；上限一天，避免模型给出离谱数值后去解远端视频。
            "at" => value
                .as_f64()
                .is_some_and(|n| n.is_finite() && (0.0..=86_400.0).contains(&n)),
            "reason" => value
                .as_str()
                .is_some_and(|text| text.chars().count() <= 200),
            "screenshots" | "forceTranscribe" => value.is_boolean(),
            "offset" => value.as_u64().is_some_and(|n| n <= usize::MAX as u64),
            "limit" => value.as_u64().is_some_and(|n| (1..=12000).contains(&n)),
            _ => false,
        };
        if !valid {
            return Err(CommandError::validation("视频工具参数类型或范围无效"));
        }
    }
    Ok(())
}

/// 分页工具只读取当前会话的视频任务，不启动新的任务。
pub(super) fn read_transcript_tool<'a, 'b>(
    context: &'a ToolContext<'b>,
    args: &'a Value,
) -> AgentFuture<'a, ToolOutcome> {
    Box::pin(async move {
        let value = context
            .video
            .read_transcript(
                string(args, "taskId"),
                args["offset"].as_u64().unwrap_or(0) as usize,
                args["limit"].as_u64().unwrap_or(6000) as usize,
            )
            .await?;
        Ok(ToolOutcome {
            value,
            ..Default::default()
        })
    })
}

/// 视频工具必须走异步端口，同步占位只用于满足注册表结构。
pub(super) fn video_sync(_: &ToolContext<'_>, _: &Value) -> Result<ToolOutcome, CommandError> {
    Err(CommandError::validation("视频工具必须异步执行"))
}

/// 取字：返回标题、来源与转录摘要，成文交给模型自己完成。
pub(super) fn transcript_tool<'a, 'b>(
    context: &'a ToolContext<'b>,
    args: &'a Value,
) -> AgentFuture<'a, ToolOutcome> {
    Box::pin(async move {
        let data = context
            .video
            .run_options(string(args, "url"), false, args)
            .await?;
        Ok(ToolOutcome {
            value: data.clone(),
            message: Some(block("video", "视频转录", data)),
            pause: false,
            generation: None,
            images: Vec::new(),
        })
    })
}

/// 画面：抽取指定秒数的截图回传给模型，由模型判断这帧能不能用。
///
/// Base64 只随本轮历史进入模型上下文，不写进任务记录或交换块。
pub(super) fn shot_tool<'a, 'b>(
    context: &'a ToolContext<'b>,
    args: &'a Value,
) -> AgentFuture<'a, ToolOutcome> {
    Box::pin(async move {
        let at = args["at"].as_f64().unwrap_or_default();
        let mut data = context.video.shot(string(args, "taskId"), at).await?;
        let images = take_image(&mut data);
        Ok(ToolOutcome {
            value: data.clone(),
            message: Some(block("video", "视频画面", data)),
            pause: false,
            generation: None,
            images,
        })
    })
}

/// 取出工具结果里的图片字段并转成历史用图片；返回值不再携带 Base64。
fn take_image(data: &mut Value) -> Vec<ToolImage> {
    let Some(image) = data.get("image").cloned() else {
        return Vec::new();
    };
    if let Some(object) = data.as_object_mut() {
        object.remove("image");
    }
    let Some(data_base64) = image.get("dataBase64").and_then(Value::as_str) else {
        return Vec::new();
    };
    vec![ToolImage {
        mime: image
            .get("mimeType")
            .and_then(Value::as_str)
            .unwrap_or("image/jpeg")
            .to_string(),
        data_base64: data_base64.to_string(),
    }]
}

/// 成文：生成结构化笔记并落盘，界面直接给出打开入口。
pub(super) fn note_tool<'a, 'b>(
    context: &'a ToolContext<'b>,
    args: &'a Value,
) -> AgentFuture<'a, ToolOutcome> {
    Box::pin(async move {
        let data = context
            .video
            .run_options(string(args, "url"), true, args)
            .await?;
        Ok(ToolOutcome {
            value: data.clone(),
            message: Some(block("video", "视频笔记", data)),
            pause: false,
            generation: None,
            images: Vec::new(),
        })
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    /// 分 P 和开关接受原生 JSON 类型，不接受字符串伪装数值。
    #[test]
    fn accepts_options_and_rejects_invalid_ranges() {
        assert!(validate_video_args(&json!({"url":"BV1", "pages":[2,3],"quality":80,"screenshots":false,"forceTranscribe":true})).is_ok());
        for args in [
            json!({"pages":[0]}),
            json!({"pages":["2"]}),
            json!({"quality":-1}),
            json!({"screenshots":"false"}),
            json!({"limit":12001}),
        ] {
            assert!(validate_video_args(&args).is_err());
        }
    }

    #[test]
    /// 画面秒数接受小数与整数，拒绝字符串、负数与超长备注。
    fn accepts_shot_seconds_and_reason() {
        assert!(validate_video_args(&json!({"taskId":"t","at":12.5,"reason":"换一帧"})).is_ok());
        assert!(validate_video_args(&json!({"taskId":"t","at":0})).is_ok());
        for args in [
            json!({"taskId":"t","at":"12.5"}),
            json!({"taskId":"t","at":-1}),
            json!({"taskId":"t","at":90000}),
            json!({"taskId":"t","reason":"字".repeat(201)}),
        ] {
            assert!(validate_video_args(&args).is_err());
        }
    }
}
