import type { CardKind, CardSource } from "./types";

/** 固定生成来源，摘要按 UTF-8 编码的 LF 正文计算 SHA-256。 */
export interface GenerationContext {
  vaultPath: string;
  notePath: string;
  noteHash: string;
  selection: CardSource | null;
}

/** 卡片生成命令输入，数量为上限，-1 表示自动。 */
export interface GenerationInput {
  typeId: string;
  studyModeId: string;
  noteTitle: string;
  sourceText: string;
  images?: Array<{ name: string; mimeType: string; dataBase64: string }>;
  requestedCount: number;
  context?: GenerationContext;
}

/** 草稿字段原样流转，UUID 在采纳后继续作为卡片身份。 */
export interface GeneratedCard {
  draftId: string;
  fields: Record<string, string>;
  source: CardSource | null;
}

/** 完成、停止或部分失败均可携带有效草稿。 */
export interface GenerationResult {
  cards: GeneratedCard[];
  warnings: string[];
}

/** 后端任务登记完成后才返回此标识。 */
export interface GenerationTaskStart { taskId: string }

/** 状态查询与取消共享同一契约，只有终态携带最终草稿。 */
export interface GenerationTaskStatus {
  taskId: string;
  state: "running" | "completed" | "cancelled" | "failed";
  phase: "preparing" | "planning" | "generating" | "lookup" | "validating";
  generatedCount: number;
  result: GenerationResult | null;
  error: { code: string; message: string } | null;
}

/** 采纳必须校验原 Vault 与正文版本，不能随当前选中笔记漂移。 */
export interface AdoptCardsInput {
  expectedVaultPath: string;
  expectedNoteHash: string;
  notePath: string;
  kind: CardKind;
  cards: GeneratedCard[];
}

/** 新增、重试已存在与内容重复均明确回传。 */
export interface AdoptCardsResult {
  addedIds: string[];
  existingIds: string[];
  duplicateIds: string[];
}
