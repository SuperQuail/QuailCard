import { ref } from "vue";
import { activeNoteCards } from "../services/stores/cardStore";
import { activeNotePath, findNote } from "../services/stores/noteStore";

/** 复习会话状态。 */
export interface ReviewSession {
  open: boolean;
  title: string;
  notePath: string | null;
  includeAll: boolean;
}

/** 复习会话用例：从当前笔记或全局今日队列进入复习。 */
export function useReviewSessionFlow(options: { showToast: (message: string) => void }) {
  const reviewSession = ref<ReviewSession>({ open: false, title: "", notePath: null, includeAll: false });

  /** 开始复习当前笔记（无卡片时提示）。 */
  function startReviewFromNote(): void {
    if (activeNoteCards.value.length === 0) {
      options.showToast("这篇笔记还没有卡片，先拆几张吧");
      return;
    }
    const title = findNote(activeNotePath.value)?.title ?? "";
    reviewSession.value = { open: true, title: `复习 · ${title}`, notePath: activeNotePath.value, includeAll: true };
  }

  /** 开始全局今日复习。 */
  function startTodayReview(): void {
    reviewSession.value = { open: true, title: "今日复习", notePath: null, includeAll: false };
  }

  return { reviewSession, startReviewFromNote, startTodayReview };
}
