import { computed, reactive, ref } from "vue";
import * as backend from "../api/backend";
import { getReviewQueue, refreshStats } from "./stores/reviewStore";
import type { AiEvaluationResult, ReviewCard, ReviewRating } from "../domain/types";
import { resolveError } from "../utils/errorMessage";

export interface ReviewSnapshot {
  queue: ReviewCard[]; index: number; finished: boolean;
  stats: Record<ReviewRating, number>; completedIds: string[];
  pendingRatings?: Record<string, ReviewRating>;
  pendingAnswers?: Record<string, string>;
  outcomes?: Array<{ question: string; notePath: string; rating: ReviewRating; answer: string }>;
}
export interface ReviewSessionOptions {
  paths: string[]; includeAll: boolean; id: string; saved?: ReviewSnapshot;
  persist?: (snapshot: ReviewSnapshot) => Promise<void>;
}

/** 原复习窗口与 Agent 共用真实队列、评分、幂等键和完成统计。 */
export function createReviewSession(options: ReviewSessionOptions) {
  const loading = ref(false), errorMessage = ref(""), busy = ref(false);
  const queue = ref<ReviewCard[]>(options.saved?.queue ?? []);
  const index = ref(options.saved?.index ?? 0), finished = ref(options.saved?.finished ?? false);
  const stats = reactive<Record<ReviewRating, number>>(options.saved?.stats ?? { again: 0, hard: 0, good: 0 });
  const completedIds = new Set(options.saved?.completedIds ?? []);
  const pendingRatings = { ...options.saved?.pendingRatings };
  const pendingAnswers = { ...options.saved?.pendingAnswers };
  const outcomes = ref(options.saved?.outcomes ?? []);
  const currentCard = computed(() => queue.value[index.value] ?? null);
  let loaded = Boolean(options.saved), round = 0;

  /** UI 状态持久化不执行评分，重开会话只能恢复已经发生的用户动作。 */
  async function persist(): Promise<void> {
    await options.persist?.({ queue: queue.value, index: index.value, finished: finished.value, stats: { ...stats }, completedIds: [...completedIds], pendingRatings, pendingAnswers, outcomes: outcomes.value });
  }
  /** 队列在开始时固定，恢复时保留原版本以保证评分重试幂等。 */
  async function loadQueue(): Promise<void> {
    if (loaded) {
      if (!finished.value && currentCard.value && completedIds.has(currentCard.value.id)) {
        try { await nextCard(); } catch (error) { errorMessage.value = resolveError(error); }
      }
      return;
    }
    if (loading.value) return;
    loading.value = true; errorMessage.value = "";
    try {
      const batches = await Promise.all((options.paths.length ? options.paths : [null]).map(path => getReviewQueue(path, options.includeAll)));
      queue.value = [...new Map(batches.flat().map(card => [card.id, card])).values()];
      loaded = true;
      await persist();
    } catch (error) { errorMessage.value = resolveError(error); }
    finally { loading.value = false; }
  }
  /** 当前卡片与版本共同组成幂等身份，同一次提交重试不能换键。 */
  function operation(card: ReviewCard): string { return `${options.id}:${round}:${card.id}:${card.version}`; }
  /** 先记录评分事实再推进界面，重复成功响应不会重复计数。 */
  async function record(card: ReviewCard, rating: ReviewRating, answer = ""): Promise<void> {
    if (!completedIds.has(card.id)) {
      completedIds.add(card.id); stats[rating] += 1;
      outcomes.value.push({ question: card.front, notePath: card.notePath, rating, answer });
    }
    await persist();
    await refreshStats();
  }
  /** 自评分仅由用户点击触发，错误保留当前卡片可重试。 */
  async function rate(rating: ReviewRating): Promise<void> {
    const card = currentCard.value;
    if (!card || busy.value || finished.value) return;
    busy.value = true; errorMessage.value = "";
    try {
      const selected = pendingRatings[card.id] ?? rating;
      pendingRatings[card.id] = selected;
      await persist();
      await backend.submitReview(card.id, selected, card.version, operation(card));
      await record(card, selected);
      await nextCard();
    } catch (error) { errorMessage.value = resolveError(error); }
    finally { busy.value = false; }
  }
  /** AI 判定复用后端原子评分，重试保持与当前卡片相同的身份。 */
  async function evaluate(cardId: string, answer: string, version: number): Promise<AiEvaluationResult> {
    const card = currentCard.value;
    if (!card || card.id !== cardId || card.version !== version) throw new Error("当前复习卡片已变化");
    if (busy.value || finished.value) throw new Error("正在处理当前复习提交");
    busy.value = true;
    try {
      const submitted = pendingAnswers[card.id] ?? answer;
      pendingAnswers[card.id] = submitted;
      await persist();
      const result = await backend.evaluateAnswer(card.id, submitted, card.version, operation(card));
      await record(card, result.isCorrect ? "good" : "again", submitted);
      return result;
    } finally { busy.value = false; }
  }
  /** 推进只改变会话位置，未作答时不允许跳过生成虚假完成记录。 */
  async function nextCard(): Promise<void> {
    if (!currentCard.value || !completedIds.has(currentCard.value.id)) return;
    if (index.value + 1 >= queue.value.length) finished.value = true;
    else index.value += 1;
    await persist();
  }
  /** 再来一轮重新取得卡片版本，禁止重复提交上一轮快照。 */
  async function restartRound(): Promise<void> {
    round += 1; loaded = false; index.value = 0; finished.value = false; completedIds.clear();
    outcomes.value = [];
    for (const key of Object.keys(pendingRatings)) delete pendingRatings[key];
    for (const key of Object.keys(pendingAnswers)) delete pendingAnswers[key];
    Object.assign(stats, { again: 0, hard: 0, good: 0 });
    await loadQueue();
  }
  return { loading, errorMessage, busy, queue, index, finished, stats, outcomes, currentCard, loadQueue, rate, evaluate, nextCard, restartRound };
}
export type ReviewSessionFlow = ReturnType<typeof createReviewSession>;
