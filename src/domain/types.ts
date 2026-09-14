/** 卡片类型：听写、自评问答、AI 判定问答。 */
export type { AgentSession, AgentMessage, AgentInput, AgentRun, AgentChange, AgentMemory } from "./agent";

/** 卡片类型：旧数据的 ai 继续兼容，新增问答采用独立评分方式。 */
export type CardKind = "vocabulary" | "qa" | "ai";

/** 复习评分。 */
export type ReviewRating = "again" | "hard" | "good";

/** 笔记摘要：对应 Vault 中的一个 .md 文件。 */
export interface NoteSummary {
  path: string;
  title: string;
  tagsJson: string;
  cardCount: number;
  dueCount: number;
  mtime: number;
}

/** 读取到的笔记文件内容。 */
export interface NoteFile {
  path: string;
  title: string;
  content: string;
  /** 磁盘修改时间（秒），用于检测外部修改。 */
  mtime: number;
}

/** 卡片面板展示的单张卡片及调度状态。 */
export interface CardSource {
  from: number;
  to: number;
  excerpt: string;
  prefix: string;
  suffix: string;
}

/** 划词身份与来源，用于验证弹窗期间正文是否改变。 */
export interface CardSelection {
  notePath: string;
  source: CardSource;
}

/** 卡片面板展示的单张卡片及调度状态。 */
export interface NoteCard {
  id: string;
  notePath: string;
  sourceRef: string;
  source?: CardSource | null;
  kind: CardKind;
  front: string;
  back: string;
  detail: string;
  example: string;
  aliases: string[];
  rubricPoints: string[];
  position: number;
  schedulerPhase: string;
  dueAt: number;
  intervalDays: number;
  totalReviews: number;
  version: number;
}

/** 保存或更新单张卡片的输入。 */
export interface CardInput {
  id?: string | null;
  notePath: string;
  sourceRef?: string | null;
  source?: CardSource | null;
  kind: CardKind;
  front: string;
  back: string;
  detail?: string | null;
  example?: string | null;
  aliases?: string[];
  rubric?: string[];
}

/** 复习队列中的卡片。 */
export interface ReviewCard {
  id: string;
  notePath: string;
  sourceRef: string;
  kind: CardKind;
  front: string;
  back: string;
  detail: string;
  example: string;
  aliases: string[];
  rubricPoints: string[];
  state: string;
  version: number;
}

/** 听写判定结果。 */
export interface DictationResult {
  correct: boolean;
  expected: string;
  aliases: string[];
}

/** 提交评分后的最新调度状态。 */
export interface ReviewProgress {
  dueAt: number;
  intervalDays: number;
  repetitions: number;
  lapses: number;
  totalReviews: number;
  version: number;
  schedulerPhase: string;
  stability: number | null;
  difficulty: number;
}

/** AI 判定结果。 */
export interface AiEvaluationResult {
  isCorrect: boolean;
  feedback: string;
  missingPoints: string[];
  suggestedAnswer: string;
  progress: ReviewProgress | null;
}

/** 全文搜索的命中。 */
export interface NoteHit {
  path: string;
  title: string;
  snippet: string;
}

/** 卡片搜索命中。 */
export interface CardHit {
  cardId: string;
  notePath: string;
  front: string;
  snippet: string;
}

/** 全局搜索结果。 */
export interface SearchResult {
  notes: NoteHit[];
  cards: CardHit[];
}

/** 学习统计。 */
export interface StudyStats {
  dueCount: number;
  totalCards: number;
  weeklyCompletionRate: number | null;
  weeklyCompletedCount: number;
}

/**
 * 供应商模型目录中的一项。
 *
 * 契约：id 是真正发给供应商的模型名；name 只用于界面显示。
 * contextWindow / maxOutputTokens 为空表示沿用后端默认（输出上限默认 32768）。
 */
export interface ProviderModel {
  id: string;
  name: string;
  contextWindow: number | null;
  maxOutputTokens: number | null;
}

/** 供应商摘要。 */
export interface ProviderSummary {
  id: string;
  name: string;
  shortCode: string;
  protocol: string;
  model: string;
  /** 模型目录：至少一项，model 指向其中当前使用的那一项。 */
  models: ProviderModel[];
  baseUrl: string;
  hasApiKey: boolean;
  hasCredential: boolean;
  authType: string | null;
  oauthAccountId: string | null;
  providerType: string;
  supportsVision: boolean;
  status: string;
}

/** 应用启动数据。 */
export interface BootstrapData {
  notes: NoteSummary[];
  providers: ProviderSummary[];
  activeProviderId: string;
  studyStats: StudyStats;
  fontSize: FontSizeId;
  vaultPath: string | null;
  recentVaults: string[];
  aiGradingEnabled: boolean;
}

/** 当前 Vault 的文件级配置。 */
export interface VaultConfig {
  attachmentFolder: string;
}

/** 导入图片附件的命令输入。 */
export interface ImportNoteAttachmentInput {
  notePath: string;
  fileName: string;
  mimeType: string;
  dataBase64: string;
}

/** 导入完成后可写入 Markdown 的相对路径。 */
export interface ImportedAttachment {
  markdownPath: string;
}

/** 读取到的图片附件载荷。 */
export interface AttachmentImage {
  mimeType: string;
  dataBase64: string;
}

/** 界面字号档位。 */
export type FontSizeId = "compact" | "standard" | "comfortable" | "large";

/** 数据位置信息：Vault 内卡片目录与应用配置目录。 */
export interface DataLocations {
  cardsDir: string | null;
  configDir: string;
}

/** 可在文件管理器中打开的数据目录目标。 */
export type DataFolderTarget = "cards" | "config";

/** 主题。 */
export type ThemeId = "light" | "dark";

/** 保险库保护状态。 */
export interface VaultStatus {
  protectionMode: "default" | "password";
  locked: boolean;
}

/** 供应商保存输入。 */
export interface ProviderInput {
  id?: string | null;
  name: string;
  shortCode: string;
  protocol: string;
  /** 当前使用的模型 id，必须是 models 中某一项的 id。 */
  model: string;
  /** 模型目录整表替换；空数组时后端按 model 合成单项。 */
  models: ProviderModel[];
  baseUrl: string;
  supportsVision: boolean;
  apiKey?: string | null;
}

/** 连接测试结果。 */
export interface ConnectionTestResult {
  latencyMs: number;
  provider: ProviderSummary | null;
}

/** OpenAI 登录启动信息。 */
export interface OpenAiLoginStart {
  attemptId: string;
  mode: string;
  url: string;
  userCode: string | null;
}

/** OpenAI 登录状态。 */
export interface OpenAiLoginStatus {
  status: string;
  message: string;
  provider: ProviderSummary | null;
}

export type { AdoptCardsInput, AdoptCardsResult, GeneratedCard, GenerationContext, GenerationInput, GenerationResult, GenerationTaskStart, GenerationTaskStatus } from "./generation";
