use std::path::PathBuf;

use tauri::State;
use zeroize::Zeroize;

use crate::{
    database::Database,
    dictionary,
    error::CommandError,
    models::{
        AdoptCardsInput, AiEvaluationResult, BootstrapData, CardInput, ConnectionTestResult,
        DictationInput, DictationResult, EvaluateAnswerInput, GenerationInput, GenerationResult,
        NoteCard, NoteFile, NoteSummary, OpenAiLoginMode, OpenAiLoginStart, OpenAiLoginStatus,
        ProviderInput, ProviderSummary, ReviewCard, ReviewProgress, SaveNoteInput, SearchResult,
        StudyStats, SubmitReviewInput, VaultStatus,
    },
    services::{AppServices, SpeechService},
    vaultfs::VaultState,
};

// ============================================================
// Vault 与笔记文件
// ============================================================

/// 选择 Vault 根目录，扫描全部笔记并重建索引。
#[tauri::command]
pub async fn open_vault(
    vault: State<'_, VaultState>,
    database: State<'_, Database>,
    path: String,
) -> Result<String, CommandError> {
    let root = vault.set_root(PathBuf::from(path))?;
    let files = vault.scan()?;
    database.rebuild_note_index(&files).await?;
    database
        .record_recent_vault(&root.to_string_lossy())
        .await?;
    Ok(root.to_string_lossy().to_string())
}

/// 查询历史打开的 Vault 路径。
#[tauri::command]
pub async fn get_recent_vaults(database: State<'_, Database>) -> Result<Vec<String>, CommandError> {
    database.get_recent_vaults().await
}

/// 返回当前 Vault 根目录路径。
#[tauri::command]
pub async fn get_vault_path(vault: State<'_, VaultState>) -> Result<Option<String>, CommandError> {
    Ok(vault.root()?.map(|path| path.to_string_lossy().to_string()))
}

/// 重新扫描 Vault 并重建索引，用于窗口聚焦时检测外部修改。
#[tauri::command]
pub async fn rescan_vault(
    vault: State<'_, VaultState>,
    database: State<'_, Database>,
) -> Result<usize, CommandError> {
    let files = vault.scan()?;
    let count = files.len();
    database.rebuild_note_index(&files).await?;
    Ok(count)
}

/// 读取笔记文件并同步其索引缓存，返回新修改时间。
#[tauri::command]
pub async fn sync_note_index(
    vault: State<'_, VaultState>,
    database: State<'_, Database>,
    path: String,
) -> Result<i64, CommandError> {
    let (content, mtime) = vault.read_note(&path)?;
    database.upsert_note_index(&path, &content, mtime).await?;
    Ok(mtime)
}

/// 查询全部笔记摘要。
#[tauri::command]
pub async fn list_notes(database: State<'_, Database>) -> Result<Vec<NoteSummary>, CommandError> {
    database.list_notes().await
}

/// 读取笔记文件内容与修改时间。
#[tauri::command]
pub async fn read_note(
    vault: State<'_, VaultState>,
    path: String,
) -> Result<NoteFile, CommandError> {
    let (content, mtime) = vault.read_note(&path)?;
    let title = path
        .rsplit(['/', '\\'])
        .next()
        .unwrap_or(&path)
        .trim_end_matches(".md")
        .to_string();
    Ok(NoteFile {
        path,
        title,
        content,
        mtime,
    })
}

/// 保存笔记文件并同步索引。
#[tauri::command]
pub async fn write_note(
    vault: State<'_, VaultState>,
    database: State<'_, Database>,
    input: SaveNoteInput,
) -> Result<i64, CommandError> {
    let mtime = vault.write_note(&input.path, &input.content)?;
    database
        .upsert_note_index(&input.path, &input.content, mtime)
        .await?;
    Ok(mtime)
}

/// 新建空白笔记文件并返回其内容。
#[tauri::command]
pub async fn create_note_file(
    vault: State<'_, VaultState>,
    database: State<'_, Database>,
    folder: String,
    title: String,
) -> Result<NoteFile, CommandError> {
    let path = vault.create_note(&folder, &title)?;
    let (content, mtime) = vault.read_note(&path)?;
    database.upsert_note_index(&path, &content, mtime).await?;
    Ok(NoteFile {
        path: path.clone(),
        title,
        content,
        mtime,
    })
}

/// 新建文件夹。
#[tauri::command]
pub async fn create_folder(vault: State<'_, VaultState>, path: String) -> Result<(), CommandError> {
    vault.create_folder(&path)
}

/// 重命名笔记文件并同步索引与卡片路径。
#[tauri::command]
pub async fn rename_note_file(
    vault: State<'_, VaultState>,
    database: State<'_, Database>,
    old_path: String,
    new_path: String,
) -> Result<String, CommandError> {
    let renamed = vault.rename_note(&old_path, &new_path)?;
    database.rename_note_paths(&old_path, &renamed).await?;
    Ok(renamed)
}

/// 删除笔记文件及其全部卡片。
#[tauri::command]
pub async fn delete_note_file(
    vault: State<'_, VaultState>,
    database: State<'_, Database>,
    path: String,
) -> Result<(), CommandError> {
    vault.delete_note(&path)?;
    database.delete_note_paths(&path).await?;
    Ok(())
}

/// 重命名文件夹并同步其下索引与卡片路径。
#[tauri::command]
pub async fn rename_folder(
    vault: State<'_, VaultState>,
    database: State<'_, Database>,
    old_path: String,
    new_path: String,
) -> Result<String, CommandError> {
    let renamed = vault.rename_folder(&old_path, &new_path)?;
    database.rename_note_paths(&old_path, &renamed).await?;
    Ok(renamed)
}

/// 删除文件夹及其全部内容与卡片。
#[tauri::command]
pub async fn delete_folder(
    vault: State<'_, VaultState>,
    database: State<'_, Database>,
    path: String,
) -> Result<(), CommandError> {
    vault.delete_folder(&path)?;
    database.delete_folder_paths(&path).await?;
    Ok(())
}

// ============================================================
// 卡片与搜索
// ============================================================

/// 保存或更新单张卡片。
#[tauri::command]
pub async fn save_card(
    database: State<'_, Database>,
    input: CardInput,
) -> Result<NoteCard, CommandError> {
    database.save_card(input).await
}

/// 删除单张卡片。
#[tauri::command]
pub async fn delete_card(
    database: State<'_, Database>,
    card_id: String,
) -> Result<(), CommandError> {
    database.delete_card(&card_id).await
}

/// 查询指定笔记的全部卡片。
#[tauri::command]
pub async fn list_note_cards(
    database: State<'_, Database>,
    note_path: String,
) -> Result<Vec<NoteCard>, CommandError> {
    database.list_note_cards(&note_path).await
}

/// 采纳 AI 拆卡草稿并批量写入卡片。
#[tauri::command]
pub async fn adopt_cards(
    database: State<'_, Database>,
    input: AdoptCardsInput,
) -> Result<usize, CommandError> {
    database.adopt_cards(&input).await
}

/// 全文搜索笔记与卡片。
#[tauri::command]
pub async fn search(
    database: State<'_, Database>,
    query: String,
) -> Result<SearchResult, CommandError> {
    database.search(&query).await
}

// ============================================================
// 复习
// ============================================================

/// 读取复习队列：可限定笔记；include_all 包含未到期卡片。
#[tauri::command]
pub async fn get_review_queue(
    database: State<'_, Database>,
    note_path: Option<String>,
    include_all: bool,
) -> Result<Vec<ReviewCard>, CommandError> {
    database
        .get_review_queue(note_path.as_deref(), include_all)
        .await
}

/// 后端权威听写判定。
#[tauri::command]
pub async fn check_dictation(
    database: State<'_, Database>,
    input: DictationInput,
) -> Result<DictationResult, CommandError> {
    database
        .check_dictation(&input.card_id, &input.answer)
        .await
}

/// 幂等提交单张卡片评分。
#[tauri::command]
pub async fn submit_review(
    database: State<'_, Database>,
    input: SubmitReviewInput,
) -> Result<ReviewProgress, CommandError> {
    database.submit_review(input).await
}

/// 由活动供应商判定单题回答并原子记录复习结果。
#[tauri::command]
pub async fn evaluate_answer(
    database: State<'_, Database>,
    services: State<'_, AppServices>,
    input: EvaluateAnswerInput,
) -> Result<AiEvaluationResult, CommandError> {
    services.evaluate_answer(&database, input).await
}

// ============================================================
// 生成
// ============================================================

/// 使用活动供应商将学习材料生成统一卡片草稿。
#[tauri::command]
pub async fn generate_cards(
    database: State<'_, Database>,
    dictionary: State<'_, dictionary::Dictionary>,
    services: State<'_, AppServices>,
    input: GenerationInput,
) -> Result<GenerationResult, CommandError> {
    services.generate_cards(&database, &dictionary, input).await
}

// ============================================================
// 供应商、保险库与系统
// ============================================================

/// 返回应用启动所需的笔记、供应商和活动配置。
#[tauri::command]
pub async fn get_bootstrap_data(
    database: State<'_, Database>,
    vault: State<'_, VaultState>,
) -> Result<BootstrapData, CommandError> {
    Ok(BootstrapData {
        notes: database.list_notes().await?,
        providers: database.list_providers().await?,
        active_provider_id: database.get_active_provider_id().await?,
        study_stats: database.get_study_stats().await?,
        font_size: database.get_font_size().await,
        vault_path: vault.root()?.map(|path| path.to_string_lossy().to_string()),
        recent_vaults: database.get_recent_vaults().await?,
        ai_grading_enabled: database.get_ai_grading_enabled().await?,
    })
}

/// 设置并持久化界面字号档位并返回当前档位。
#[tauri::command]
pub async fn set_font_size(
    database: State<'_, Database>,
    font_size: String,
) -> Result<String, CommandError> {
    database.set_font_size(&font_size).await?;
    Ok(font_size)
}

/// 设置"使用问答时启用AI评分"。
#[tauri::command]
pub async fn set_ai_grading_enabled(
    database: State<'_, Database>,
    enabled: bool,
) -> Result<bool, CommandError> {
    database.set_ai_grading_enabled(enabled).await?;
    Ok(enabled)
}

/// 查询学习统计。
#[tauri::command]
pub async fn get_study_stats(database: State<'_, Database>) -> Result<StudyStats, CommandError> {
    database.get_study_stats().await
}

/// 设置当前活动供应商。
#[tauri::command]
pub async fn set_active_provider(
    database: State<'_, Database>,
    provider_id: String,
) -> Result<(), CommandError> {
    database.set_active_provider(&provider_id).await
}

/// 删除供应商及其加密凭据。
#[tauri::command]
pub async fn delete_provider(
    database: State<'_, Database>,
    services: State<'_, AppServices>,
    provider_id: String,
) -> Result<(), CommandError> {
    services.delete_provider(&database, &provider_id).await
}

/// 创建或更新供应商非敏感配置。
#[tauri::command]
pub async fn save_provider(
    database: State<'_, Database>,
    services: State<'_, AppServices>,
    input: ProviderInput,
) -> Result<ProviderSummary, CommandError> {
    services.save_provider(&database, input).await
}

/// 使用当前表单中的供应商配置发起真实连接测试。
#[tauri::command]
pub async fn test_provider(
    database: State<'_, Database>,
    services: State<'_, AppServices>,
    input: ProviderInput,
) -> Result<ConnectionTestResult, CommandError> {
    services.test_provider(&database, input).await
}

/// 启动 OpenAI 浏览器 PKCE 或设备码登录。
#[tauri::command]
pub async fn start_openai_login(
    database: State<'_, Database>,
    services: State<'_, AppServices>,
    provider_id: String,
    mode: OpenAiLoginMode,
) -> Result<OpenAiLoginStart, CommandError> {
    services
        .start_openai_login(&database, &provider_id, mode)
        .await
}

/// 查询 OpenAI OAuth 后台登录进度。
#[tauri::command]
pub async fn get_openai_login_status(
    services: State<'_, AppServices>,
    attempt_id: String,
) -> Result<OpenAiLoginStatus, CommandError> {
    services.get_openai_login_status(&attempt_id).await
}

/// 取消仍在等待用户授权的 OpenAI OAuth 登录。
#[tauri::command]
pub async fn cancel_openai_login(
    services: State<'_, AppServices>,
    attempt_id: String,
) -> Result<OpenAiLoginStatus, CommandError> {
    services.cancel_openai_login(&attempt_id).await
}

/// 注销 OpenAI OAuth 并清理加密保险库凭据。
#[tauri::command]
pub async fn logout_openai(
    database: State<'_, Database>,
    services: State<'_, AppServices>,
    provider_id: String,
) -> Result<ProviderSummary, CommandError> {
    services.logout_openai(&database, &provider_id).await
}

/// 查询加密凭据保险库的保护模式和锁定状态。
#[tauri::command]
pub async fn get_vault_status(
    database: State<'_, Database>,
    services: State<'_, AppServices>,
) -> Result<VaultStatus, CommandError> {
    services.get_vault_status(&database).await
}

/// 使用临时密码参数解锁保险库并立即清零参数内存。
#[tauri::command]
pub async fn unlock_vault(
    database: State<'_, Database>,
    services: State<'_, AppServices>,
    mut password: String,
) -> Result<VaultStatus, CommandError> {
    let result = services.unlock_vault(&database, &password).await;
    password.zeroize();
    result
}

/// 设置或更换保险库密码并立即清零参数内存。
#[tauri::command]
pub async fn set_vault_password(
    database: State<'_, Database>,
    services: State<'_, AppServices>,
    mut password: String,
) -> Result<VaultStatus, CommandError> {
    let result = services.set_vault_password(&database, &password).await;
    password.zeroize();
    result
}

/// 移除用户密码并恢复默认加密保护。
#[tauri::command]
pub async fn remove_vault_password(
    database: State<'_, Database>,
    services: State<'_, AppServices>,
) -> Result<VaultStatus, CommandError> {
    services.remove_vault_password(&database).await
}

/// 手动清除当前会话中的密码派生密钥。
#[tauri::command]
pub async fn lock_vault(
    database: State<'_, Database>,
    services: State<'_, AppServices>,
) -> Result<VaultStatus, CommandError> {
    services.lock_vault(&database).await
}

/// 忘记密码时丢弃全部凭据并恢复默认保护。
#[tauri::command]
pub async fn reset_vault(
    database: State<'_, Database>,
    services: State<'_, AppServices>,
) -> Result<VaultStatus, CommandError> {
    services.reset_vault(&database).await
}

/// 查询内置 ECDICT 词典中的单词条目。
#[tauri::command]
pub async fn lookup_dictionary_word(
    dictionary: State<'_, dictionary::Dictionary>,
    word: String,
) -> Result<Option<dictionary::DictionaryEntry>, CommandError> {
    dictionary.lookup(&word).await
}

/// 合成单词发音并返回可播放的音频 Data URL。
#[tauri::command]
pub async fn synthesize_speech(
    speech: State<'_, SpeechService>,
    text: String,
) -> Result<String, CommandError> {
    speech.synthesize(&text).await
}
