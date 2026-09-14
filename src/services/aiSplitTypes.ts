import type { CardKind, CardSource, GeneratedCard, GenerationTaskStatus } from "../domain/types";

/** 打开对话框时冻结，切换笔记不会改变草稿的采纳目标。 */
export interface AiSplitSnapshot {
  vaultPath: string;
  notePath: string;
  noteTitle: string;
  noteContent: string;
  kind: CardKind;
  selection: CardSource | null;
}

/** 拆卡用例独占可变状态，展示组件仅消费此窄视图。 */
export interface AiSplitState {
  open: boolean;
  snapshot: AiSplitSnapshot | null;
  step: "scope" | "running" | "drafts";
  scope: "note" | "selection";
  requestedCount: number;
  phase: GenerationTaskStatus["phase"];
  generatedCount: number;
  taskId: string | null;
  stopping: boolean;
  saving: boolean;
  drafts: GeneratedCard[];
  accepted: Set<string>;
  warnings: string[];
  invalidReason: string;
}

/** 每次会话都新建集合，禁止旧响应污染下一次打开的对话框。 */
export function emptyAiSplitState(): AiSplitState {
  return {
    open: false, snapshot: null, step: "scope", scope: "note", requestedCount: -1,
    phase: "preparing", generatedCount: 0, taskId: null, stopping: false, saving: false,
    drafts: [], accepted: new Set(), warnings: [], invalidReason: "",
  };
}
