//! 卡片文件的持久化记录结构：卡片 + 调度状态 + 复习历史同文件。
//!
//! 演化规则：新增字段必须带默认值；调度算法引入新参数时在
//! ReviewStateRecord 上增加可选字段即可，旧卡片文件无需任何改写。

use serde::{Deserialize, Serialize};

use super::envelope;
use crate::models::{NoteCard, ReviewProgress};

/// 单篇笔记的卡片文件完整内容。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub(crate) struct NoteCardsFile {
    /// 信封版本，缺失按当前版本解析。
    pub(crate) format_version: u64,
    /// 所属笔记的 Vault 相对路径，仅供人工核对；权威值以文件位置为准。
    pub(crate) note_path: String,
    /// 该笔记的全部卡片，按 position 升序。
    pub(crate) cards: Vec<CardRecord>,
}

impl Default for NoteCardsFile {
    /// 容错读取的空文件骨架。
    fn default() -> Self {
        Self {
            format_version: envelope::CURRENT_FORMAT_VERSION,
            note_path: String::new(),
            cards: Vec::new(),
        }
    }
}

/// 单张卡片的完整记录，含调度状态与复习历史。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub(crate) struct CardRecord {
    /// 卡片全局唯一标识（UUID v7）。
    pub(crate) id: String,
    /// 卡片类型：vocabulary / qa（保持开放字符串以支持未来类型）。
    pub(crate) kind: String,
    pub(crate) front: String,
    pub(crate) back: String,
    pub(crate) detail: String,
    pub(crate) example: String,
    /// 来源引用（如笔记内小节标题）。
    pub(crate) source_ref: String,
    /// 听写判定的可接受别名。
    pub(crate) aliases: Vec<String>,
    /// AI 判定的评分要点。
    pub(crate) rubric_points: Vec<String>,
    /// 同笔记内的顺序号。
    pub(crate) position: i64,
    pub(crate) created_at: i64,
    pub(crate) updated_at: i64,
    /// 复习调度状态，新卡为默认值（立即到期）。
    pub(crate) review: ReviewStateRecord,
    /// 复习历史，含幂等键与 AI 判定原文。
    pub(crate) history: Vec<HistoryRecord>,
}

impl Default for CardRecord {
    /// 容错读取的默认值：空问答卡、全新调度状态。
    fn default() -> Self {
        Self {
            id: String::new(),
            kind: "qa".to_string(),
            front: String::new(),
            back: String::new(),
            detail: String::new(),
            example: String::new(),
            source_ref: String::new(),
            aliases: Vec::new(),
            rubric_points: Vec::new(),
            position: 0,
            created_at: 0,
            updated_at: 0,
            review: ReviewStateRecord::default(),
            history: Vec::new(),
        }
    }
}

/// 自适应调度器的持久化状态快照。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub(crate) struct ReviewStateRecord {
    pub(crate) due_at: i64,
    pub(crate) interval_days: i64,
    pub(crate) repetitions: i64,
    pub(crate) lapses: i64,
    pub(crate) total_reviews: i64,
    pub(crate) last_result: Option<String>,
    /// 乐观锁版本号，提交评分时校验。
    pub(crate) version: i64,
    pub(crate) stability: Option<f64>,
    pub(crate) difficulty: f64,
    pub(crate) last_review_at: Option<i64>,
    /// new / learning / review / relearning。
    pub(crate) scheduler_phase: String,
    pub(crate) learning_step: i64,
}

impl Default for ReviewStateRecord {
    /// 新卡默认：立即到期、难度 5.0、全新阶段。
    fn default() -> Self {
        Self {
            due_at: 0,
            interval_days: 0,
            repetitions: 0,
            lapses: 0,
            total_reviews: 0,
            last_result: None,
            version: 0,
            stability: None,
            difficulty: 5.0,
            last_review_at: None,
            scheduler_phase: "new".to_string(),
            learning_step: 0,
        }
    }
}

/// 单次复习的历史记录。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub(crate) struct HistoryRecord {
    pub(crate) id: String,
    /// again / hard / good。
    pub(crate) result: String,
    /// 本次复习前卡片所在的到期时间。
    pub(crate) scheduled_due_at: i64,
    pub(crate) reviewed_at: i64,
    /// 全局唯一幂等键，防止重复提交。
    pub(crate) idempotency_key: String,
    /// AI 判定原文（无 schema 的自由 JSON），普通复习为 None。
    pub(crate) ai_evaluation: Option<serde_json::Value>,
}

impl Default for HistoryRecord {
    /// 容错读取的空记录。
    fn default() -> Self {
        Self {
            id: String::new(),
            result: String::new(),
            scheduled_due_at: 0,
            reviewed_at: 0,
            idempotency_key: String::new(),
            ai_evaluation: None,
        }
    }
}

impl CardRecord {
    /// 转换为卡片面板展示模型。
    pub(crate) fn to_note_card(&self, note_path: &str) -> NoteCard {
        NoteCard {
            id: self.id.clone(),
            note_path: note_path.to_string(),
            source_ref: self.source_ref.clone(),
            kind: self.kind.clone(),
            front: self.front.clone(),
            back: self.back.clone(),
            detail: self.detail.clone(),
            example: self.example.clone(),
            aliases: self.aliases.clone(),
            rubric_points: self.rubric_points.clone(),
            position: self.position,
            scheduler_phase: self.review.scheduler_phase.clone(),
            due_at: self.review.due_at,
            interval_days: self.review.interval_days,
            total_reviews: self.review.total_reviews,
            version: self.review.version,
        }
    }

    /// 转换为最新调度进度。
    pub(crate) fn to_progress(&self) -> ReviewProgress {
        ReviewProgress {
            due_at: self.review.due_at,
            interval_days: self.review.interval_days,
            repetitions: self.review.repetitions,
            lapses: self.review.lapses,
            total_reviews: self.review.total_reviews,
            version: self.review.version,
            scheduler_phase: self.review.scheduler_phase.clone(),
            stability: self.review.stability,
            difficulty: self.review.difficulty,
        }
    }
}
