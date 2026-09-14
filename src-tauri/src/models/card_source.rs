use serde::{Deserialize, Serialize};

/// 与笔记原文关联的 UTF-16 选区快照。
#[derive(Debug, Clone, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", default)]
pub struct CardSource {
    /// 编辑器 UTF-16 偏移，只在摘录与上下文吻合时用于定位。
    pub from: usize,
    pub to: usize,
    pub excerpt: String,
    pub prefix: String,
    pub suffix: String,
}
