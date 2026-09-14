import { ref, watch } from "vue";
import { vaultPath } from "../services/stores/vaultStore";
import { activeNoteCards } from "../services/stores/cardStore";
import { activeNotePath, findNote } from "../services/stores/noteStore";

/** 复习会话状态。 */
export interface ReviewSession {
  id?: string;
  open: boolean;
  title: string;
  notePath: string | null;
  includeAll: boolean;
}

/** 复习会话用例：从当前笔记或全局今日队列进入复习。 */
export function useReviewSessionFlow(options: { showToast: (message: string) => void }) {
  const reviewSession = ref<ReviewSession>({ open: false, title: "", notePath: null, includeAll: false });

  /** 换库后丢弃旧队列，避免在另一知识库继续评分。 */
  watch(vaultPath, () => {
    reviewSession.value = { open: false, title: "", notePath: null, includeAll: false };
  });

  /** 开始复习当前笔记（无卡片时提示）。 */
  function startReviewFromNote(): void {
    if (reviewSession.value.id && reviewSession.value.notePath === activeNotePath.value && reviewSession.value.includeAll) {
      reviewSession.value.open = true;
      return;
    }
    if (activeNoteCards.value.length === 0) {
      options.showToast("这篇笔记还没有卡片，先拆几张吧");
      return;
    }
    const title = findNote(activeNotePath.value)?.title ?? "";
    reviewSession.value = { id: crypto.randomUUID(), open: true, title: `复习 · ${title}`, notePath: activeNotePath.value, includeAll: true };
  }

  /** 工作区入口优先恢复当前会话，首次进入才加载今日队列。 */
  function startTodayReview(): void {
    if (reviewSession.value.id) {
      reviewSession.value.open = true;
      return;
    }
    reviewSession.value = { id: crypto.randomUUID(), open: true, title: "今日复习", notePath: null, includeAll: false };
  }

  return { reviewSession, startReviewFromNote, startTodayReview };
}
