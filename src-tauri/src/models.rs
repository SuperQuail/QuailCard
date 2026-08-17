use std::collections::HashMap;

/// OpenAI 订阅登录供应商的稳定子类型。
pub const OPENAI_SUBSCRIPTION_PROVIDER_TYPE: &str = "openai_subscription";
/// OpenAI 订阅登录内置供应商的稳定标识。
pub const OPENAI_SUBSCRIPTION_PROVIDER_ID: &str = "openai_subscription";
/// OpenAI 订阅请求不会被用户配置覆盖的固定端点。
pub const OPENAI_SUBSCRIPTION_ENDPOINT: &str = "https://chatgpt.com/backend-api/codex/responses";
use serde::{Deserialize, Serialize};

// ============================================================
// 笔记与卡片
// ============================================================

/// 笔记摘要：对应 Vault 中的一个 .md 文件。
#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
#[serde(rename_all = "camelCase")]
pub struct NoteSummary {
    pub path: String,
    pub title: String,
    pub tags_json: String,
    pub card_count: i64,
    pub due_count: i64,
    pub mtime: i64,
}

/// 读取到的笔记文件内容。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NoteFile {
    pub path: String,
    pub title: String,
    pub content: String,
    /// 磁盘修改时间（秒），用于检测外部修改。
    pub mtime: i64,
}

/// 当前 Vault 独立保存的文件附件配置。
#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct VaultConfig {
    pub attachment_folder: String,
}

/// 导入笔记图片附件时使用的完整输入。
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ImportNoteAttachmentInput {
    pub note_path: String,
    pub file_name: String,
    pub mime_type: String,
    pub data_base64: String,
}

/// 导入成功后可直接写入 Markdown 的相对图片路径。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ImportedAttachment {
    pub markdown_path: String,
}

/// 从笔记中的 Markdown 图片来源读取附件。
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReadNoteAttachmentInput {
    pub note_path: String,
    pub source: String,
}

/// 经实际文件签名验证后返回的图片载荷。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AttachmentImage {
    pub mime_type: String,
    pub data_base64: String,
}

/// 保存笔记文件的输入：内容写盘并同步索引。
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SaveNoteInput {
    pub path: String,
    pub content: String,
}

/// 复习队列中的卡片。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ReviewCard {
    pub id: String,
    pub note_path: String,
    pub source_ref: String,
    pub kind: String,
    pub front: String,
    pub back: String,
    pub detail: String,
    pub example: String,
    pub aliases: Vec<String>,
    pub rubric_points: Vec<String>,
    /// new / learning / review / relearning
    pub state: String,
    pub version: i64,
}

/// 卡片面板展示的单张卡片及调度状态。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NoteCard {
    pub id: String,
    pub note_path: String,
    pub source_ref: String,
    pub kind: String,
    pub front: String,
    pub back: String,
    pub detail: String,
    pub example: String,
    pub aliases: Vec<String>,
    pub rubric_points: Vec<String>,
    pub position: i64,
    pub scheduler_phase: String,
    pub due_at: i64,
    pub interval_days: i64,
    pub total_reviews: i64,
    pub version: i64,
}

/// 保存或更新单张卡片的输入。
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CardInput {
    pub id: Option<String>,
    pub note_path: String,
    pub source_ref: Option<String>,
    pub kind: String,
    pub front: String,
    pub back: String,
    pub detail: Option<String>,
    pub example: Option<String>,
    #[serde(default)]
    pub aliases: Vec<String>,
    #[serde(default)]
    pub rubric: Vec<String>,
}

/// 采纳 AI 拆卡草稿：卡片与来源一同写入。
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AdoptCardsInput {
    pub note_path: String,
    pub kind: String,
    pub cards: Vec<GeneratedCard>,
}

/// 听写判定的输入。
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DictationInput {
    pub card_id: String,
    pub answer: String,
}

/// 听写判定的结果。
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DictationResult {
    pub correct: bool,
    pub expected: String,
    pub aliases: Vec<String>,
}

/// 全文搜索的笔记命中。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NoteHit {
    pub path: String,
    pub title: String,
    pub snippet: String,
}

/// 全文搜索的卡片命中。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CardHit {
    pub card_id: String,
    pub note_path: String,
    pub front: String,
    pub snippet: String,
}

/// 全局搜索的合并结果。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SearchResult {
    pub notes: Vec<NoteHit>,
    pub cards: Vec<CardHit>,
}

// ============================================================
// 复习
// ============================================================

/// 用户对普通学习卡的评分。
#[derive(Debug, Clone, Copy, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ReviewRating {
    Again,
    Hard,
    Good,
}

impl ReviewRating {
    /// 返回持久化使用的评分字符串。
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Again => "again",
            Self::Hard => "hard",
            Self::Good => "good",
        }
    }
}

/// 提交复习结果的幂等输入。
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SubmitReviewInput {
    pub card_id: String,
    pub rating: ReviewRating,
    pub expected_version: i64,
    pub idempotency_key: String,
}

/// 提交评分后的最新调度状态。
#[derive(Debug, Clone, Deserialize, Serialize, sqlx::FromRow)]
#[serde(rename_all = "camelCase")]
pub struct ReviewProgress {
    pub due_at: i64,
    pub interval_days: i64,
    pub repetitions: i64,
    pub lapses: i64,
    pub total_reviews: i64,
    pub version: i64,
    pub scheduler_phase: String,
    pub stability: Option<f64>,
    pub difficulty: f64,
}

/// AI 问答提交的独立请求。
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EvaluateAnswerInput {
    pub card_id: String,
    pub user_answer: String,
    pub expected_version: i64,
    pub idempotency_key: String,
    #[serde(default)]
    pub practice: bool,
}

/// AI 判定所需且只从数据库读取的卡片上下文。
#[derive(Debug)]
pub struct AiEvaluationContext {
    pub question: String,
    pub reference_answer: String,
    pub rubric_points: Vec<String>,
}

/// 单题 AI 判定及已提交的调度结果。
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AiEvaluationResult {
    pub is_correct: bool,
    pub feedback: String,
    pub missing_points: Vec<String>,
    pub suggested_answer: String,
    pub progress: Option<ReviewProgress>,
}

// ============================================================
// 供应商与保险库
// ============================================================

/// 模型供应商的非敏感摘要。
#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
#[serde(rename_all = "camelCase")]
pub struct ProviderSummary {
    pub id: String,
    pub name: String,
    pub short_code: String,
    pub protocol: String,
    pub model: String,
    pub base_url: String,
    pub has_api_key: bool,
    pub has_credential: bool,
    pub auth_type: Option<String>,
    pub oauth_account_id: Option<String>,
    pub provider_type: String,
    pub supports_vision: bool,
    pub status: String,
}

/// 保存供应商非敏感配置的输入。
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderInput {
    pub id: Option<String>,
    pub name: String,
    pub short_code: String,
    pub protocol: String,
    pub model: String,
    pub base_url: String,
    pub supports_vision: bool,
    pub api_key: Option<String>,
}

/// 后端发起模型请求所需的供应商配置。
#[derive(Debug, Clone, sqlx::FromRow)]
pub struct ProviderConfig {
    pub id: String,
    pub protocol: String,
    pub model: String,
    pub base_url: String,
    pub secret_ref: Option<String>,
    pub auth_type: Option<String>,
    pub oauth_account_id: Option<String>,
    pub provider_type: String,
    pub supports_vision: bool,
}

/// 加密保险库内保存的 OpenAI OAuth 凭据。
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OpenAiOAuthCredential {
    pub access_token: String,
    pub refresh_token: String,
    pub expires_at: i64,
    pub account_id: Option<String>,
}

/// OpenAI OAuth 登录方式。
#[derive(Debug, Clone, Copy, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OpenAiLoginMode {
    Browser,
    Device,
}

/// 启动 OpenAI 登录后返回给界面的信息。
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OpenAiLoginStart {
    pub attempt_id: String,
    pub mode: String,
    pub url: String,
    pub user_code: Option<String>,
}

/// 查询 OpenAI 登录进度的结果。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OpenAiLoginStatus {
    pub status: String,
    pub message: String,
    pub provider: Option<ProviderSummary>,
}

/// SQLite 中保存的认证加密保险库密文记录。
#[derive(Debug, Clone, sqlx::FromRow)]
pub struct VaultEnvelope {
    pub format_version: i64,
    pub protection_mode: String,
    pub kdf_salt: Vec<u8>,
    pub kdf_iterations: Option<i64>,
    pub nonce: Vec<u8>,
    pub ciphertext: Vec<u8>,
}

/// 前端可见的保险库保护状态。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VaultStatus {
    pub protection_mode: String,
    pub locked: bool,
}

/// 连接测试的耗时结果。
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ConnectionTestResult {
    pub latency_ms: u64,
    pub provider: Option<ProviderSummary>,
}

// ============================================================
// 生成与启动数据
// ============================================================

/// 卡片生成命令的输入：材料来自笔记正文或用户粘贴文本。
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GenerationInput {
    pub type_id: String,
    pub study_mode_id: String,
    pub note_title: String,
    pub source_text: String,
    #[serde(default)]
    pub images: Vec<GenerationImage>,
    pub requested_count: i32,
}

/// 用户明确选择并发送给模型的笔记图片。
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GenerationImage {
    pub name: String,
    pub mime_type: String,
    pub data_base64: String,
}

/// 模型生成的一张统一字段草稿。
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GeneratedCard {
    pub fields: HashMap<String, String>,
}

/// 卡片生成命令返回的可编辑结果。
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GenerationResult {
    pub cards: Vec<GeneratedCard>,
    pub warnings: Vec<String>,
}

/// 应用启动时一次返回的基础数据。
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BootstrapData {
    pub notes: Vec<NoteSummary>,
    pub providers: Vec<ProviderSummary>,
    pub active_provider_id: String,
    pub study_stats: StudyStats,
    pub font_size: String,
    pub vault_path: Option<String>,
    pub recent_vaults: Vec<String>,
    pub ai_grading_enabled: bool,
}

/// 学习统计。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StudyStats {
    pub due_count: i64,
    pub total_cards: i64,
    pub weekly_completion_rate: Option<i64>,
    pub weekly_completed_count: i64,
}
