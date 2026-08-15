import { ref } from "vue";
import * as backend from "../../api/backend";
import type {
  AiEvaluationResult,
  DictationResult,
  ReviewCard,
  ReviewProgress,
  ReviewRating,
  StudyStats,
} from "../../domain/types";

/**
 * 复习域 store：学习统计、复习队列、听写判定、AI 评分与语音合成。
 * 自成一体，不依赖其他 store；note/card 域在数据变化后会调用 refreshStats 同步统计。
 */
export const studyStats = ref<StudyStats>({ dueCount: 0, totalCards: 0, weeklyCompletionRate: null, weeklyCompletedCount: 0 });
/** 问答卡是否启用 AI 评分（全局设置，随启动数据下发）。 */
export const aiGradingEnabled = ref(false);

/** 只刷新统计，避免整页闪烁。 */
export async function refreshStats(): Promise<void> {
  studyStats.value = await backend.getStudyStats();
}

/** 读取复习队列。 */
export function getReviewQueue(notePath: string | null, includeAll: boolean): Promise<ReviewCard[]> {
  return backend.getReviewQueue(notePath, includeAll);
}

/** 后端听写判定。 */
export function checkDictation(cardId: string, answer: string): Promise<DictationResult> {
  return backend.checkDictation(cardId, answer);
}

/** 提交评分：会话 ID 由前端生成，保证重试幂等。 */
export async function submitReview(cardId: string, rating: ReviewRating, expectedVersion: number): Promise<ReviewProgress> {
  return backend.submitReview(cardId, rating, expectedVersion, crypto.randomUUID());
}

/** AI 判定回答。 */
export async function evaluateAnswer(cardId: string, userAnswer: string, expectedVersion: number): Promise<AiEvaluationResult> {
  return backend.evaluateAnswer(cardId, userAnswer, expectedVersion, crypto.randomUUID());
}

/** 合成单词发音（失败时返回空串由前端回退浏览器 TTS）。 */
export function synthesizeSpeech(text: string): Promise<string> {
  return backend.synthesizeSpeech(text);
}

/** 设置"使用问答时启用AI评分"。 */
export async function setAiGradingEnabled(enabled: boolean): Promise<void> {
  aiGradingEnabled.value = await backend.setAiGradingEnabled(enabled);
}
