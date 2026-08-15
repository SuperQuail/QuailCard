import { ref } from "vue";
import * as backend from "../../api/backend";
import type { CardInput, GenerationInput, GenerationResult, NoteCard } from "../../domain/types";
import { activeNotePath } from "./noteStore";
import { refreshStats } from "./reviewStore";

/**
 * 卡片域 store：当前笔记的卡片列表与选中卡片。
 * 依赖 noteStore 提供当前笔记路径，依赖 reviewStore 在卡片变化后同步统计。
 */
export const activeNoteCards = ref<NoteCard[]>([]);
export const activeCardId = ref<string | null>(null);

/** 加载指定笔记（缺省为当前笔记）的卡片并清除选中。 */
export async function loadActiveCards(path?: string): Promise<void> {
  const target = path ?? activeNotePath.value;
  if (!target) {
    clearActiveCards();
    return;
  }
  activeNoteCards.value = await backend.listNoteCards(target);
  activeCardId.value = null;
}

/** 清空卡片状态（笔记关闭或离开 Vault 时）。 */
export function clearActiveCards(): void {
  activeNoteCards.value = [];
  activeCardId.value = null;
}

/** 重新加载当前笔记的卡片（保留现有语义，不额外清空选中）。 */
export async function reloadActiveCards(): Promise<void> {
  if (activeNotePath.value) {
    activeNoteCards.value = await backend.listNoteCards(activeNotePath.value);
  }
}

/** 保存单张卡片（新建或更新）并同步统计。 */
export async function saveCard(input: CardInput): Promise<NoteCard> {
  const card = await backend.saveCard(input);
  await reloadActiveCards();
  await refreshStats();
  return card;
}

/** 删除单张卡片：若是选中卡片则清除选中，并同步统计。 */
export async function deleteCard(cardId: string): Promise<void> {
  await backend.deleteCard(cardId);
  if (activeCardId.value === cardId) {
    activeCardId.value = null;
  }
  await reloadActiveCards();
  await refreshStats();
}

/** 采纳 AI 拆卡草稿并同步统计。 */
export async function adoptCards(input: { notePath: string; kind: string; cards: Array<{ fields: Record<string, string> }> }): Promise<void> {
  await backend.adoptCards(input);
  await reloadActiveCards();
  await refreshStats();
}

/** 使用活动供应商生成卡片草稿。 */
export function generateCards(input: GenerationInput): Promise<GenerationResult> {
  return backend.generateCards(input);
}
