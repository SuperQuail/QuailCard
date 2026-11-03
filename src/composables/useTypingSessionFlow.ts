import { ref, watch } from "vue";
import { vaultPath } from "../services/stores/vaultStore";
import { activeNoteCards } from "../services/stores/cardStore";
import { activeNotePath, findNote } from "../services/stores/noteStore";
import type { TypingScope } from "../services/typingSession";

/** 打字背诵会话状态；id 作为工作区组件的 key，重建即新会话。 */
export interface TypingSessionFlowState {
  id?: string;
  open: boolean;
  title: string;
  scope: TypingScope;
  notePath: string | null;
}

/** 打字背诵入口：从当前笔记的单词卡或整个 Vault 进入。 */
export function useTypingSessionFlow(options: { showToast: (message: string) => void }) {
  const typingSession = ref<TypingSessionFlowState>({ open: false, title: "", scope: "all", notePath: null });

  /** 换库后丢弃旧词池，避免在另一知识库继续练习。 */
  watch(vaultPath, () => {
    typingSession.value = { open: false, title: "", scope: "all", notePath: null };
  });

  /** 从当前笔记进入：没有单词卡时提示，不打开工作区。 */
  function startTypingFromNote(): void {
    const hasVocabulary = activeNoteCards.value.some((card) => card.kind === "vocabulary");
    if (!hasVocabulary) {
      options.showToast("这篇笔记还没有单词卡，先拆几张吧");
      return;
    }
    if (typingSession.value.id && typingSession.value.notePath === activeNotePath.value && typingSession.value.scope === "note") {
      typingSession.value.open = true;
      return;
    }
    const title = findNote(activeNotePath.value)?.title ?? "";
    typingSession.value = { id: crypto.randomUUID(), open: true, title: `打字背诵 · ${title}`, scope: "note", notePath: activeNotePath.value };
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
