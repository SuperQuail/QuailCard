import { invoke } from "@tauri-apps/api/core";
import type {
  AiEvaluationResult,
  AttachmentImage,
  BootstrapData,
  CardInput,
  ConnectionTestResult,
  DictationResult,
  FontSizeId,
  GenerationInput,
  GenerationResult,
  ImportNoteAttachmentInput,
  ImportedAttachment,
  NoteCard,
  NoteFile,
  NoteSummary,
  OpenAiLoginStart,
  OpenAiLoginStatus,
  ProviderInput,
  ProviderSummary,
  ReviewCard,
  ReviewProgress,
  ReviewRating,
  SearchResult,
  StudyStats,
  VaultStatus,
  VaultConfig,
} from "../domain/types";

/** 判断当前是否运行在 Tauri WebView 内。 */
export function isTauri(): boolean {
  return typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;
}

/**
 * 统一命令调用：Tauri 环境走 IPC，浏览器环境回退内存演示。
 *
 * 契约：`call` 保持与后端命令同名同参，前端用例只依赖本文件导出函数，
 * 不关心底层是 IPC 还是演示实现。
 *
 * 说明：演示后端已移至 src/dev/mockBackend/，这里用动态 import 按需加载，
 * 避免 Tauri 生产构建把演示代码打进主包。
 */
async function call<T>(command: string, args?: Record<string, unknown>): Promise<T> {
  if (isTauri()) {
    return invoke<T>(command, args);
  }
  const mock = await import("../dev/mockBackend");
  return mock.call<T>(command, args);
}

/** 从后端错误对象中提取用户可读消息。 */
export function resolveErrorMessage(error: unknown): string {
  if (typeof error === "object" && error !== null && "message" in error) {
    return String(error.message);
  }
  return String(error);
}

/** 选择 Vault 根目录并重建索引。 */
export function openVault(path: string): Promise<string> {
  return call<string>("open_vault", { path });
}

/** 查询当前 Vault 根目录。 */
export function getVaultPath(): Promise<string | null> {
  return call<string | null>("get_vault_path");
}

/** 查询应用启动数据。 */
export function getBootstrapData(): Promise<BootstrapData> {
  return call<BootstrapData>("get_bootstrap_data");
}

/** 查询当前 Vault 的附件配置。 */
export function getVaultConfig(): Promise<VaultConfig> {
  return call<VaultConfig>("get_vault_config");
}

/** 保存当前 Vault 的附件目录。 */
export function setAttachmentFolder(attachmentFolder: string): Promise<VaultConfig> {
  return call<VaultConfig>("set_attachment_folder", { attachmentFolder });
}

/** 将图片写入当前 Vault 并返回相对 Markdown 路径。 */
export function importNoteAttachment(input: ImportNoteAttachmentInput): Promise<ImportedAttachment> {
  return call<ImportedAttachment>("import_note_attachment", { input });
}

/** 读取当前笔记引用的本地图片。 */
export function readNoteAttachment(notePath: string, source: string): Promise<AttachmentImage> {
  return call<AttachmentImage>("read_note_attachment", { input: { notePath, source } });
}

/** 查询全部笔记摘要。 */
export function listNotes(): Promise<NoteSummary[]> {
  return call<NoteSummary[]>("list_notes");
}

/** 读取笔记文件内容。 */
export function readNote(path: string): Promise<NoteFile> {
  return call<NoteFile>("read_note", { path });
}

/** 保存笔记文件并同步索引。 */
export function writeNote(path: string, content: string): Promise<number> {
  return call<number>("write_note", { input: { path, content } });
}

/** 新建空白笔记文件。 */
export function createNoteFile(folder: string, title: string): Promise<NoteFile> {
  return call<NoteFile>("create_note_file", { folder, title });
}

/** 新建文件夹。 */
export function createFolder(path: string): Promise<void> {
  return call<void>("create_folder", { path });
}

/** 重命名笔记文件。 */
export function renameNoteFile(oldPath: string, newPath: string): Promise<string> {
  return call<string>("rename_note_file", { oldPath, newPath });
}

/** 删除笔记文件及其卡片。 */
export function deleteNoteFile(path: string): Promise<void> {
  return call<void>("delete_note_file", { path });
}

/** 重命名文件夹。 */
export function renameFolder(oldPath: string, newPath: string): Promise<string> {
  return call<string>("rename_folder", { oldPath, newPath });
}

/** 删除文件夹及其内容。 */
export function deleteFolder(path: string): Promise<void> {
  return call<void>("delete_folder", { path });
}

/** 保存或更新单张卡片。 */
export function saveCard(input: CardInput): Promise<NoteCard> {
  return call<NoteCard>("save_card", { input });
}

/** 删除单张卡片。 */
export function deleteCard(cardId: string): Promise<void> {
  return call<void>("delete_card", { cardId });
}

/** 查询指定笔记的全部卡片。 */
export function listNoteCards(notePath: string): Promise<NoteCard[]> {
  return call<NoteCard[]>("list_note_cards", { notePath });
}

/** 采纳 AI 拆卡草稿。 */
export function adoptCards(input: { notePath: string; kind: string; cards: Array<{ fields: Record<string, string> }> }): Promise<number> {
  return call<number>("adopt_cards", { input });
}

/** 全文搜索笔记与卡片。 */
export function search(query: string): Promise<SearchResult> {
  return call<SearchResult>("search", { query });
}

/** 读取复习队列。 */
export function getReviewQueue(notePath: string | null, includeAll: boolean): Promise<ReviewCard[]> {
  return call<ReviewCard[]>("get_review_queue", { notePath, includeAll });
}

/** 后端权威听写判定。 */
export function checkDictation(cardId: string, answer: string): Promise<DictationResult> {
  return call<DictationResult>("check_dictation", { input: { cardId, answer } });
}

/** 幂等提交评分。 */
export function submitReview(cardId: string, rating: ReviewRating, expectedVersion: number, idempotencyKey: string): Promise<ReviewProgress> {
  return call<ReviewProgress>("submit_review", { input: { cardId, rating, expectedVersion, idempotencyKey } });
}

/** AI 判定单题回答。 */
export function evaluateAnswer(cardId: string, userAnswer: string, expectedVersion: number, idempotencyKey: string): Promise<AiEvaluationResult> {
  return call<AiEvaluationResult>("evaluate_answer", { input: { cardId, userAnswer, expectedVersion, idempotencyKey } });
}

/** 查询学习统计。 */
export function getStudyStats(): Promise<StudyStats> {
  return call<StudyStats>("get_study_stats");
}

/** 设置全局字号档位。 */
export function setFontSize(fontSize: FontSizeId): Promise<string> {
  return call<string>("set_font_size", { fontSize });
}

/** 设置"使用问答时启用AI评分"。 */
export function setAiGradingEnabled(enabled: boolean): Promise<boolean> {
  return call<boolean>("set_ai_grading_enabled", { enabled });
}

/** 查询供应商列表。 */
export function listProviders(): Promise<ProviderSummary[]> {
  return call<ProviderSummary[]>("list_providers");
}

/** 设置当前活动供应商。 */
export function setActiveProvider(providerId: string): Promise<void> {
  return call<void>("set_active_provider", { providerId });
}

/** 查询保险库状态。 */
export function getVaultStatus(): Promise<VaultStatus> {
  return call<VaultStatus>("get_vault_status");
}

/** 设置保险库密码。 */
export function setVaultPassword(password: string): Promise<VaultStatus> {
  return call<VaultStatus>("set_vault_password", { password });
}

/** 使用活动供应商生成卡片草稿。 */
export function generateCards(input: GenerationInput): Promise<GenerationResult> {
  return call<GenerationResult>("generate_cards", { input });
}

/** 读取笔记并同步其索引（检测外部修改后调用）。 */
export function syncNoteIndex(path: string): Promise<number> {
  return call<number>("sync_note_index", { path });
}

/** 查询历史打开的 Vault 路径。 */
export function getRecentVaults(): Promise<string[]> {
  return call<string[]>("get_recent_vaults");
}

/** 重新扫描 Vault 并重建索引。 */
export function rescanVault(): Promise<number> {
  return call<number>("rescan_vault");
}

/** 保存供应商配置。 */
export function saveProvider(input: ProviderInput): Promise<ProviderSummary> {
  return call<ProviderSummary>("save_provider", { input });
}

/** 删除供应商及其凭据。 */
export function deleteProvider(providerId: string): Promise<void> {
  return call<void>("delete_provider", { providerId });
}

/** 测试供应商连接。 */
export function testProvider(input: ProviderInput): Promise<ConnectionTestResult> {
  return call<ConnectionTestResult>("test_provider", { input });
}

/** 启动 OpenAI 登录。 */
export function startOpenAiLogin(providerId: string, mode: "browser" | "device"): Promise<OpenAiLoginStart> {
  return call<OpenAiLoginStart>("start_openai_login", { providerId, mode });
}

/** 查询 OpenAI 登录状态。 */
export function getOpenAiLoginStatus(attemptId: string): Promise<OpenAiLoginStatus> {
  return call<OpenAiLoginStatus>("get_openai_login_status", { attemptId });
}

/** 取消 OpenAI 登录。 */
export function cancelOpenAiLogin(attemptId: string): Promise<OpenAiLoginStatus> {
  return call<OpenAiLoginStatus>("cancel_openai_login", { attemptId });
}

/** 注销 OpenAI 登录。 */
export function logoutOpenAi(providerId: string): Promise<ProviderSummary> {
  return call<ProviderSummary>("logout_openai", { providerId });
}

/** 合成单词发音，返回音频 Data URL。 */
export function synthesizeSpeech(text: string): Promise<string> {
  return call<string>("synthesize_speech", { text });
}
