import { ref } from "vue";
import { resolveError } from "../utils/errorMessage";
import { activeNoteCards, activeCardId, deleteCard, saveCard } from "../services/stores/cardStore";
import { activeNoteContent, activeNotePath, notePersistence, saveNoteContent } from "../services/stores/noteStore";
import type { CardKind, CardSelection, CardSource } from "../domain/types";
import { isCardSourceCurrent } from "../domain/cardSource";

/** 编辑草稿保存原笔记身份，避免切换笔记后把卡片写入别处。 */
export interface CardEditorState {
  open: boolean;
  kind: CardKind;
  editingId: string | null;
  front: string;
  back: string;
  detail: string;
  example: string;
  rubric: string;
  notePath: string | null;
  source: CardSource | null;
  sourceRef: string;
  aliases: string[];
}

/** 创建空白表单，划词来源另行显式传入。 */
function createEmptyCardEditor(kind: CardKind): CardEditorState {
  return { open: false, kind, editingId: null, front: "", back: "", detail: "", example: "", rubric: "",
    notePath: null, source: null, sourceRef: "", aliases: [] };
}

/** 判定要点继续沿用现有分隔符契约。 */
function splitRubric(text: string): string[] {
  return text.split(/[、,，;；\n]/).map((item) => item.trim()).filter(Boolean);
}

/** 手动拆卡只写卡片与来源数据，不重新序列化笔记。 */
export function useCardEditorFlow(options: { showToast: (message: string) => void }) {
  const cardEditor = ref<CardEditorState>(createEmptyCardEditor("qa"));
  const cardSaving = ref(false);
  let reselectingSource = false;

  /** 新建或编辑表单绑定打开时的笔记。 */
  function openCardEditor(kind: CardKind, initial: Partial<CardEditorState> = {}): void {
    reselectingSource = false;
    cardEditor.value = { ...createEmptyCardEditor(kind), notePath: activeNotePath.value, ...initial, open: true };
  }

  /** 选区统一是答案，不按语言猜测。 */
  function openCardEditorFromSelection(selection: CardSelection): void {
    if (reselectingSource && cardEditor.value.notePath === selection.notePath) {
      cardEditor.value.source = selection.source;
      cardEditor.value.open = true;
      reselectingSource = false;
      return;
    }
    openCardEditor(activeNoteCards.value[0]?.kind ?? "qa", {
      front: "", back: selection.source.excerpt, source: selection.source, notePath: selection.notePath,
    });
  }

  /** 暂存完整表单后回到原文，让用户重新选择来源而不丢问题和答案。 */
  function reselectCardSource(draft: Pick<CardEditorState, "kind" | "front" | "back" | "detail" | "example" | "rubric">): void {
    Object.assign(cardEditor.value, draft, { open: false });
    reselectingSource = true;
    options.showToast("草稿已保留，请在原笔记中重新选中文字并点击拆成卡片");
  }

  /** 保存前校验来源并等待原文落盘；失败保持弹窗和草稿。 */
  async function handleCardEditorSave(draft: { kind: CardKind; front: string; back: string; detail: string; example: string; rubric: string }): Promise<void> {
    const editor = cardEditor.value;
    if (!editor.notePath || cardSaving.value) return;
    Object.assign(editor, draft);
    cardSaving.value = true;
    try {
      if (editor.source && !editor.editingId) {
        if (activeNotePath.value !== editor.notePath || !isCardSourceCurrent(activeNoteContent.value, editor.source)) {
          throw new Error("原文选区已改变，草稿已保留，请重新选择来源");
        }
        await notePersistence.flush(editor.notePath);
      }
      const card = await saveCard({
        id: editor.editingId, notePath: editor.notePath, source: editor.source, sourceRef: editor.sourceRef,
        aliases: editor.aliases, kind: draft.kind, front: draft.front, back: draft.back,
        detail: draft.detail, example: draft.example, rubric: splitRubric(draft.rubric),
      });
      activeCardId.value = card.id;
      options.showToast(editor.editingId ? "卡片已更新" : "卡片已创建");
      editor.open = false;
    } catch (error) {
      options.showToast(resolveError(error));
    } finally {
      cardSaving.value = false;
    }
  }

  /** 已有来源、别名及评分要点必须随编辑保存，不因表单未展示而丢失。 */
  function editCard(cardId: string): void {
    const card = activeNoteCards.value.find((item) => item.id === cardId);
    if (!card) return;
    activeCardId.value = cardId;
    openCardEditor(card.kind, { editingId: card.id, front: card.front, back: card.back,
      detail: card.detail, example: card.example, rubric: card.rubricPoints.join("、"),
      source: card.source ?? null, sourceRef: card.sourceRef, aliases: card.aliases, notePath: card.notePath });
  }

  /** 旧卡只清理自身标记，不再归一化全文空白；新卡无需改动原文。 */
  async function handleDeleteCard(cardId: string): Promise<void> {
    const marker = "^qc-" + cardId;
    const content = activeNoteContent.value;
    const nextContent = content.split(marker).join("");
    if (activeNotePath.value && nextContent !== content) await saveNoteContent(activeNotePath.value, nextContent);
    await deleteCard(cardId);
  }

  return { cardEditor, cardSaving, reselectCardSource, openCardEditor, openCardEditorFromSelection, handleCardEditorSave, editCard, handleDeleteCard };
}
