import { ref, watch } from "vue";
import { vaultPath } from "../services/stores/vaultStore";
import { activeNoteCards } from "../services/stores/cardStore";
import type { NoteCard } from "../domain/types";
import { activeNotePath, findNote } from "../services/stores/noteStore";
import { isWordGroupPath } from "../domain/documentKind";
import type { TypingScope } from "../services/typingSession";

/** 打字背诵会话状态；id 作为工作区组件的 key，重建即新会话。 */
export interface TypingSessionFlowState {
  id?: string;
  open: boolean;
  title: string;
  scope: TypingScope;
  notePath: string | null;
  /** 建会话时的单词卡指纹：卡片增删改后词池会过期，指纹不一致就重建会话。 */
  cardKey?: string;
}

/** 单词卡指纹：id 与四个展示字段任一变化，都说明练习词池已经不是最新的。 */
function vocabularyKey(cards: NoteCard[]): string {
  return cards
    .filter((card) => card.kind === "vocabulary")
    .map((card) => [card.id, card.front, card.back, card.detail, card.example].join("\u0001"))
    .join("\u0002");
}

/** 打字背诵入口：从当前笔记的单词卡或整个 Vault 进入。 */
export function useTypingSessionFlow(options: { showToast: (message: string) => void }) {
  const typingSession = ref<TypingSessionFlowState>({ open: false, title: "", scope: "all", notePath: null });

  /** 换库后丢弃旧词池，避免在另一知识库继续练习。 */
  watch(vaultPath, () => {
    typingSession.value = { open: false, title: "", scope: "all", notePath: null };
  });

  /** 从当前文档进入：单词组与 Markdown 笔记共用同一份卡片列表，只是空态说法不同。 */
  function startTypingFromNote(): void {
    const hasVocabulary = activeNoteCards.value.some((card) => card.kind === "vocabulary");
    if (!hasVocabulary) {
      options.showToast(isWordGroupPath(activeNotePath.value ?? "") ? "这个单词组还没有已保存的单词，先填几页吧" : "这篇笔记还没有单词卡，先拆几张吧");
      return;
    }
    // 单词组保存才物化卡片：卡片变了还复用旧会话就会练到已删或已改的单词。
    const cardKey = vocabularyKey(activeNoteCards.value);
    if (typingSession.value.id && typingSession.value.notePath === activeNotePath.value && typingSession.value.scope === "note" && typingSession.value.cardKey === cardKey) {
      typingSession.value.open = true;
      return;
    }
    const title = findNote(activeNotePath.value)?.title ?? "";
    typingSession.value = { id: crypto.randomUUID(), open: true, title: `打字背诵 · ${title}`, scope: "note", notePath: activeNotePath.value, cardKey };
  }

  /** 从丝带 / 命令面板进入：使用整个 Vault 的单词卡。 */
  function startTypingAll(): void {
    if (typingSession.value.id && typingSession.value.scope === "all") {
      typingSession.value.open = true;
      return;
    }
    typingSession.value = { id: crypto.randomUUID(), open: true, title: "打字背诵 · 全部单词", scope: "all", notePath: null };
  }

  return { typingSession, startTypingFromNote, startTypingAll };
}
