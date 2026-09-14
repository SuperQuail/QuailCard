use tauri::State;

use crate::{
    dictionary,
    error::CommandError,
    models::{
        AdoptCardsInput, AdoptCardsResult, AiEvaluationResult, CardInput, DictationInput,
        DictationResult, EvaluateAnswerInput, GenerationInput, GenerationResult, NoteCard,
        ReviewCard, ReviewProgress, SearchResult, SubmitReviewInput,
    },
    services::AppServices,
    storage::Storage,
};

// ============================================================
// 卡片与搜索
// ============================================================

/// 保存或更新单张卡片。
#[tauri::command]
pub async fn save_card(
    storage: State<'_, Storage>,
    input: CardInput,
) -> Result<NoteCard, CommandError> {
    storage.save_card(input).await
}

/// 删除单张卡片。
#[tauri::command]
pub async fn delete_card(storage: State<'_, Storage>, card_id: String) -> Result<(), CommandError> {
    storage.delete_card(&card_id).await
}

/// 查询指定笔记的全部卡片。
#[tauri::command]
pub async fn list_note_cards(
    storage: State<'_, Storage>,
    note_path: String,
) -> Result<Vec<NoteCard>, CommandError> {
    storage.list_note_cards(&note_path).await
}

/// 采纳 AI 拆卡草稿并批量写入卡片。
#[tauri::command]
pub async fn adopt_cards(
    storage: State<'_, Storage>,
    input: AdoptCardsInput,
) -> Result<AdoptCardsResult, CommandError> {
    storage.adopt_cards(&input).await
}

/// 全文搜索笔记与卡片。
#[tauri::command]
pub async fn search(
    storage: State<'_, Storage>,
    query: String,
) -> Result<SearchResult, CommandError> {
    storage.search(&query).await
}

// ============================================================
// 复习
// ============================================================

/// 读取复习队列：可限定笔记；include_all 包含未到期卡片。
#[tauri::command]
pub async fn get_review_queue(
    storage: State<'_, Storage>,
    note_path: Option<String>,
    include_all: bool,
) -> Result<Vec<ReviewCard>, CommandError> {
    storage
        .get_review_queue(note_path.as_deref(), include_all)
        .await
}

/// 后端权威听写判定。
#[tauri::command]
pub async fn check_dictation(
    storage: State<'_, Storage>,
    input: DictationInput,
) -> Result<DictationResult, CommandError> {
    storage.check_dictation(&input.card_id, &input.answer).await
}

/// 幂等提交单张卡片评分。
#[tauri::command]
pub async fn submit_review(
    storage: State<'_, Storage>,
    input: SubmitReviewInput,
) -> Result<ReviewProgress, CommandError> {
    storage.submit_review(input).await
}

/// 由活动供应商判定单题回答并原子记录复习结果。
#[tauri::command]
pub async fn evaluate_answer(
    storage: State<'_, Storage>,
    services: State<'_, AppServices>,
    input: EvaluateAnswerInput,
) -> Result<AiEvaluationResult, CommandError> {
    services.evaluate_answer(&storage, input).await
}

// ============================================================
// 生成
// ============================================================

/// 使用活动供应商将学习材料生成统一卡片草稿。
#[tauri::command]
pub async fn generate_cards(
    storage: State<'_, Storage>,
    dictionary: State<'_, dictionary::Dictionary>,
    services: State<'_, AppServices>,
    input: GenerationInput,
) -> Result<GenerationResult, CommandError> {
    services.generate_cards(&storage, &dictionary, input).await
}
