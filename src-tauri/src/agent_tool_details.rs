//! 中立工具收据的安全展示词汇；不解析 assistant、replay 或任何供应商协议。
use crate::agent_models::{AgentToolDetail, AgentToolRow, AgentToolState};
use serde_json::{Map, Value};

const MAX_COUNTER: u64 = 9_007_199_254_740_991;
const INPUT_NUMBERS: &[(&str, &str)] = &[
    ("at", "输入·时间点（秒）"),
    ("page", "输入·页码"),
    ("offset", "输入·偏移"),
    ("limit", "输入·上限"),
];
const RESULT_NUMBERS: &[(&str, &str)] = &[
    ("segments", "字幕段数"),
    ("shots", "画面数"),
    ("count", "结果数量"),
    ("offset", "结果偏移"),
    ("nextOffset", "下一偏移"),
    ("totalCharacters", "总字符数"),
    ("total", "总数"),
    ("page", "结果页码"),
    ("limit", "结果上限"),
    ("at", "画面时间（秒）"),
];
const SAFE_ERRORS: &[(&str, &str)] = &[
    ("VIDEO_FRAME_FAILED", "视频取帧失败，请换一个时间点或重试"),
    ("VIDEO_TASK_MISSING", "视频任务不存在，请重新运行视频任务"),
    (
        "VIDEO_TASK_NOT_FOUND",
        "视频任务不存在、已结束或不属于当前会话",
    ),
    ("VIDEO_NO_TRANSCRIPT", "任务转录不存在，请先完成视频转录"),
    (
        "VIDEO_TRANSCRIPT_EMPTY",
        "未获得可用字幕，请检查视频或尝试语音转录",
    ),
    (
        "VIDEO_COMPONENT_MISSING",
        "缺少媒体组件，请在设置中检查安装或路径",
    ),
    ("VIDEO_DECODE_FAILED", "视频解码失败，请检查媒体来源后重试"),
    (
        "VIDEO_ASR_FAILED",
        "Whisper 本地转录失败，请检查音频与模型文件",
    ),
    (
        "VIDEO_ASR_MODEL_MISSING",
        "Whisper 模型缺失，请先安装或选择模型",
    ),
    (
        "VIDEO_ASR_MODEL_INVALID",
        "Whisper 模型文件无效，请重新下载模型",
    ),
    (
        "VIDEO_ASR_GPU_FAILED",
        "Whisper GPU 转录失败，请尝试 CPU 模式",
    ),
    ("VIDEO_NETWORK_ERROR", "媒体网络请求失败，请检查网络后重试"),
    ("VIDEO_COMPONENT_FAILED", "媒体组件运行失败，请检查组件安装"),
    ("VIDEO_HTTP_ERROR", "视频网络请求失败，请检查网络后重试"),
    ("VIDEO_URL_INVALID", "视频地址无效，请检查后重试"),
    ("VIDEO_LOGIN_REQUIRED", "视频需要登录，请先完成登录"),
    ("VIDEO_CREDENTIAL_INVALID", "视频登录凭据无效，请重新登录"),
    ("VIDEO_API_INVALID", "视频接口返回异常，请稍后重试"),
    ("VIDEO_TASK_RUNNING", "视频任务仍在运行，请等待完成"),
    ("VIDEO_DOWNLOAD_BUSY", "视频下载繁忙，请稍后重试"),
    ("VIDEO_CANCELLED", "视频任务已取消"),
    ("VIDEO_INTERRUPTED", "视频任务执行中断，请重试"),
    ("VIDEO_NOTE_FAILED", "视频笔记生成失败，请重试"),
    ("AGENT_CANCELLED", "工具执行已停止"),
    ("AGENT_WAITING", "等待用户完成当前交互"),
    ("VALIDATION_ERROR", "工具参数或执行条件不满足，请检查后重试"),
];
const SUCCESS: &[(&str, &str)] = &[
    ("video_transcript", "视频字幕读取完成"),
    ("video_transcript_read", "字幕分页读取完成"),
    ("video_shot", "视频画面读取完成"),
    ("video_note", "视频笔记生成完成"),
    ("read_note", "笔记读取完成"),
    ("search_notes", "笔记搜索完成"),
];

/// 仅接受有界数字，不把字符串、数组或嵌套对象格式化成展示文本。
fn number(key: &str, value: &Value) -> Option<Value> {
    if key == "at" {
        return value
            .as_f64()
            .filter(|n| n.is_finite() && (0.0..=86_400.0).contains(n))
            .map(Value::from);
    }
    value
        .as_u64()
        .filter(|n| *n <= MAX_COUNTER)
        .map(Value::from)
}

/// UUID 必须可解析并重新编码；拒绝路径、签名链接和非身份自由文本。
fn task_id(value: &Value) -> Option<String> {
    let text = value.as_str().filter(|s| s.len() == 36)?;
    uuid::Uuid::parse_str(text)
        .ok()
        .map(|id| id.hyphenated().to_string())
}

/// 新中立调用只保存安全参数投影；旧协议原文绝不成为此函数的调用来源。
pub(crate) fn safe_arguments(arguments: &Value) -> Value {
    let mut safe = Map::new();
    for (key, _) in INPUT_NUMBERS {
        if let Some(value) = number(key, &arguments[*key]) {
            safe.insert((*key).into(), value);
        }
    }
    if let Some(id) = task_id(&arguments["taskId"]) {
        safe.insert("taskId".into(), Value::String(id));
    }
    Value::Object(safe)
}

/// 统一追加封闭标签，调用方必须先验证值的类型和范围。
fn detail(row: &mut AgentToolRow, label: &str, value: String) {
    row.details.push(AgentToolDetail {
        label: label.into(),
        value,
    });
}

/// 结果仅投影明确白名单计数与布尔标记，不递归遍历任意未知字段。
fn result_details(row: &mut AgentToolRow, result: &Value) {
    for (key, label) in RESULT_NUMBERS {
        if let Some(value) = number(key, &result[*key]) {
            detail(row, label, value.to_string());
        }
    }
    if let Some(id) = task_id(&result["taskId"]) {
        detail(row, "结果·任务 ID", id);
    }
    if let Some(truncated) = result["truncated"].as_bool() {
        detail(
            row,
            "结果已截断",
            if truncated { "是" } else { "否" }.into(),
        );
    }
}

/// 中立工具结果允许纯文本或单个文本块；图片块完全不读取，多文本歧义保守拒绝。
pub(super) fn receipt_text(receipt: &Value) -> Option<&str> {
    let content = &receipt["content"];
    if let Some(text) = content.as_str() {
        return Some(text);
    }
    let mut texts = content
        .as_array()?
        .iter()
        .filter(|block| block["type"] == "text");
    let text = texts.next()?["text"].as_str()?;
    if texts.next().is_some() {
        return None;
    }
    Some(text)
}

/// 只接收已唯一配对的中立结果；失败载荷只能通过静态错误码解释，正文永不回显。
pub(super) fn enrich(row: &mut AgentToolRow, arguments: &Value, receipt: Option<&Value>) {
    if row.name == "unknown_tool" {
        return;
    }
    let args = safe_arguments(arguments);
    for (key, label) in INPUT_NUMBERS {
        if let Some(value) = args.get(*key) {
            detail(row, label, value.to_string());
        }
    }
    if let Some(id) = task_id(&args["taskId"]) {
        detail(row, "输入·任务 ID", id);
    }
    let Some(receipt) = receipt else { return };
    let Some(content) =
        receipt_text(receipt).and_then(|text| serde_json::from_str::<Value>(text).ok())
    else {
        return;
    };
    if row.state == AgentToolState::Error && content["ok"] == false {
        row.summary = "工具执行失败，未提供可安全显示的原因".into();
        if let Some((code, explanation)) = content["error"]["code"]
            .as_str()
            .and_then(|code| SAFE_ERRORS.iter().find(|(known, _)| *known == code))
        {
            row.error_code = (*code).into();
            row.summary = (*explanation).into();
        }
        return;
    }
    if row.state != AgentToolState::Ok {
        return;
    }
    let result = &content["result"];
    result_details(row, result);
    // 视频取字首屏中的游标也是中立业务结果，不读取 transcript.text。
    if row.name == "video_transcript" {
        result_details(row, &result["transcript"]);
        if result["shots"].as_u64() == Some(0) {
            detail(
                row,
                "说明",
                "字幕工具不自动截图；画面数为 0 不表示取帧失败。".into(),
            );
        }
    }
    if let Some(count) = number("imageCount", &receipt["imageCount"]) {
        detail(row, "返回图片数", count.to_string());
    }
    if let Some((_, summary)) = SUCCESS.iter().find(|(name, _)| *name == row.name) {
        row.summary = (*summary).into();
    }
    if row.name == "video_transcript" {
        if let Some(count) = number("segments", &result["segments"]) {
            row.summary = format!("读取字幕 {count} 段");
        }
    } else if let Some(count) = number("count", &result["count"]) {
        row.summary = format!("工具执行完成，返回 {count} 项");
    }
}
