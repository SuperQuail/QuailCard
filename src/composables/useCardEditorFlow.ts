import { ref, type ComputedRef } from "vue";
import { attachAnchorToBlock, parseMarkdown, serializeBlocks } from "../markdown";
import { resolveError } from "../utils/errorMessage";
import { activeNoteCards, activeCardId, deleteCard, saveCard } from "../services/stores/cardStore";
import { activeNoteContent, activeNotePath, saveNoteContent } from "../services/stores/noteStore";
import type { CardKind } from "../domain/types";

/** 正文结构化块类型（与 markdown 解析结果保持同源）。 */
type Block = ReturnType<typeof parseMarkdown>[number];

/** 单卡编辑器状态。 */
export interface CardEditorState {
  open: boolean;
  kind: CardKind;
  editingId: string | null;
  front: string;
  back: string;
  detail: string;
  example: string;
  rubric: string;
  /** 划词拆卡的选区上下文：来源块与文本。 */
  blockIndex: number | null;
  selectedText: string;
}

/** 创建单卡编辑器的空白状态。 */
function createEmptyCardEditor(kind: CardKind): CardEditorState {
  return { open: false, kind, editingId: null, front: "", back: "", detail: "", example: "", rubric: "", blockIndex: null, selectedText: "" };
}

/** 拆分判定要点文本。 */
function splitRubric(text: string): string[] {
  return text.split(/[、,，;；\n]/).map((item) => item.trim()).filter(Boolean);
}

/**
 * 单卡编辑用例：打开编辑器（新建/划词/编辑）、保存（含锚点注入）、删除（含锚点清理）。
 * 依赖注入 blocks 与 showToast，保持与壳组件解耦。
 */
export function useCardEditorFlow(options: { blocks: ComputedRef<Block[]>; showToast: (message: string) => void }) {
  const cardEditor = ref<CardEditorState>(createEmptyCardEditor("qa"));

  /** 打开单卡编辑器：新建（可选预填）或编辑。 */
  function openCardEditor(kind: CardKind, initial: Partial<CardEditorState> = {}): void {
    cardEditor.value = { ...createEmptyCardEditor(kind), ...initial, open: true };
  }

  /** 划词拆卡：根据选中文本预填正反面（中文选区作正面，外文作背面）。 */
  function openCardEditorFromSelection(selection: string): void {
    const noteKind = activeNoteCards.value[0]?.kind ?? "qa";
    const containsCjk = /[\u4e00-\u9fff]/.test(selection);
    const blockIndex = options.blocks.value.findIndex((block) => "text" in block && block.text.includes(selection));
    openCardEditor(noteKind, {
      front: containsCjk ? selection : "",
      back: containsCjk ? "" : selection,
      blockIndex,
      selectedText: selection,
    });
  }

  /** 保存单卡：新建或更新；划词卡片同时在正文注入块锚点。 */
  async function handleCardEditorSave(draft: { kind: CardKind; front: string; back: string; detail: string; example: string; rubric: string }): Promise<void> {
    if (!activeNotePath.value) {
      return;
    }
    try {
      const card = await saveCard({
        id: cardEditor.value.editingId,
        notePath: activeNotePath.value,
        kind: draft.kind,
        front: draft.front,
        back: draft.back,
        detail: draft.detail,
        example: draft.example,
        rubric: splitRubric(draft.rubric),
      });
      if (cardEditor.value.blockIndex !== null && cardEditor.value.selectedText) {
        const nextBlocks = attachAnchorToBlock(options.blocks.value, cardEditor.value.blockIndex, cardEditor.value.selectedText, card.id);
        const nextContent = serializeBlocks(nextBlocks);
        await saveNoteContent(activeNotePath.value, nextContent);
        activeNoteContent.value = nextContent;
      }
      activeCardId.value = card.id;
      options.showToast("卡片已创建");
    } catch (error) {
      options.showToast(resolveError(error));
      return;
    }
    cardEditor.value.open = false;
  }

  /** 编辑已有卡片：预填表单并选中。 */
  function editCard(cardId: string): void {
    const card = activeNoteCards.value.find((item) => item.id === cardId);
    if (!card) {
      return;
    }
    activeCardId.value = cardId;
    openCardEditor(card.kind, {
      editingId: card.id,
      front: card.front,
      back: card.back,
      detail: card.detail,
      example: card.example,
      rubric: card.rubricPoints.join("、"),
    });
  }

  /** 删除卡片并移除正文锚点。 */
  async function handleDeleteCard(cardId: string): Promise<void> {
    const nextContent = activeNoteContent.value
      .replace(new RegExp(` ?\\^qc-${cardId}\\b`, "g"), "")
      .replace(/[ \t]+\n/g, "\n");
    activeNoteContent.value = nextContent;
    await saveNoteContent(activeNotePath.value ?? "", nextContent);
    await deleteCard(cardId);
    options.showToast("卡片已删除");
  }

  return { cardEditor, openCardEditor, openCardEditorFromSelection, handleCardEditorSave, editCard, handleDeleteCard };
}
