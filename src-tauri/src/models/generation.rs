use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use super::CardSource;
use crate::error::CommandError;

/// 生成任务绑定的原笔记身份；摘要按 LF 规范化的 UTF-8 正文计算。
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GenerationContext {
    pub vault_path: String,
    pub note_path: String,
    pub note_hash: String,
    pub selection: Option<CardSource>,
}

/// 旧调用可不传上下文；笔记拆卡必须携带上下文以防错绑。
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GenerationInput {
    pub type_id: String,
    pub study_mode_id: String,
    pub note_title: String,
    pub source_text: String,
    #[serde(default)]
    pub images: Vec<GenerationImage>,
    pub requested_count: i32,
    #[serde(default)]
    pub context: Option<GenerationContext>,
}

/// 用户明确选择并发送给模型的笔记图片。
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GenerationImage {
    pub name: String,
    pub mime_type: String,
    pub data_base64: String,
}

/// 草稿身份同时作为最终卡片 ID；来源只由可信原文计算。
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GeneratedCard {
    #[serde(default)]
    pub draft_id: String,
    pub fields: HashMap<String, String>,
    #[serde(default)]
    pub source: Option<CardSource>,
}

/// 成功、停止和部分失败均可携带已通过校验的结果。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GenerationResult {
    pub cards: Vec<GeneratedCard>,
    pub warnings: Vec<String>,
}

/// 启动确认只在任务已登记后返回，避免取消早于注册。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GenerationTaskStart {
    pub task_id: String,
}

/// 状态只包含真实执行阶段和校验计数，不暴露模型原始响应。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GenerationTaskStatus {
    pub task_id: String,
    pub state: String,
    pub phase: String,
    pub generated_count: usize,
    pub result: Option<GenerationResult>,
    pub error: Option<CommandError>,
}

/// 采纳绑定原 Vault 与正文版本，选中批次必须整体写入。
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AdoptCardsInput {
    pub expected_vault_path: String,
    pub expected_note_hash: String,
    pub note_path: String,
    pub kind: String,
    pub cards: Vec<GeneratedCard>,
}

/// 区分新增、同 ID 重试和内容重复，前端不得用提交数量冒充新增数。
#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AdoptCardsResult {
    pub added_ids: Vec<String>,
    pub existing_ids: Vec<String>,
    pub duplicate_ids: Vec<String>,
}
