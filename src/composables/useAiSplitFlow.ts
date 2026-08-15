import { ref } from "vue";
import { activeNoteCards, adoptCards, generateCards } from "../services/stores/cardStore";
import { activeNotePath } from "../services/stores/noteStore";

/** AI 拆卡对话框状态。 */
export interface AiSplitState {
  open: boolean;
  hasSelection: boolean;
  selectionText: string;
}

/**
 * AI 拆卡用例：携带选区打开对话框、采纳草稿。
 * 对话框内部通过 generateCards 生成草稿，本流程只负责入口与落库。
 */
export function useAiSplitFlow(options: { showToast: (message: string) => void }) {
  const aiSplit = ref<AiSplitState>({ open: false, hasSelection: false, selectionText: "" });

  /** 打开 AI 拆卡对话框，携带当前选区。 */
  function openAiSplit(): void {
    const selection = window.getSelection()?.toString().trim() ?? "";
    aiSplit.value = { open: true, hasSelection: selection.length > 0, selectionText: selection };
  }

  /** 采纳 AI 拆卡草稿（沿用笔记现有卡片类型）。 */
  async function handleAiSplitAdopt(drafts: Array<{ front: string; back: string; detail: string; example: string; rubric: string[]; source: string }>): Promise<void> {
    if (!activeNotePath.value) {
      return;
    }
    const kind = activeNoteCards.value[0]?.kind ?? "qa";
    await adoptCards({
      notePath: activeNotePath.value,
      kind,
      cards: drafts.map((draft) => ({
        fields: {
          front: draft.front,
          back: draft.back,
          detail: draft.detail,
          example: draft.example,
          rubric: draft.rubric.join("、"),
          source: draft.source,
        },
      })),
    });
    options.showToast(`已采纳 ${drafts.length} 张卡片`);
  }

  return { aiSplit, openAiSplit, handleAiSplitAdopt, generateCards };
}
