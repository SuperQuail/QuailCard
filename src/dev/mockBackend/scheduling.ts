import type { AiEvaluationResult, ReviewCard, ReviewProgress, StudyStats } from "../../domain/types";
import { cards, type MockCard } from "./state";

/**
 * 演示级“业务逻辑”。
 *
 * 重要声明：本文件所有逻辑仅用于浏览器演示，让 UI 有东西可点、可看，
 * 绝不代表真实后端业务规则。真实复习调度算法位于 src-tauri/src/scheduler.rs，
 * 真实 AI 判定流程位于 src-tauri/src/ai/ 与 services/，本文件不得复制它们。
 */

/**
 * 推进卡片调度状态（演示级简化）。
 *
 * 契约：返回领域类型 ReviewProgress，保证前端订阅能收到形状正确的数据。
 * 说明：这里用固定倍率近似间隔增长，只是为了演示界面上的“间隔天数”变化，
 * 不实现遗忘曲线、稳定性等真实调度语义。
 */
export function advanceCard(card: MockCard, rating: string): ReviewProgress {
  const now = Math.floor(Date.now() / 1000);
  card.totalReviews += 1;
  card.version += 1;
  if (rating === "again") {
    card.schedulerPhase = "relearning";
    card.intervalDays = 0;
    card.dueAt = now + 600; // 仅演示：10 分钟后重看，便于快速验证“重学”路径。
  } else {
    card.schedulerPhase = "review";
    // 仅演示：粗略翻倍，不做真实稳定性/难度建模。
    card.intervalDays = Math.max(1, card.intervalDays === 0 ? (rating === "good" ? 3 : 1) : Math.ceil(card.intervalDays * (rating === "good" ? 2.3 : 1.5)));
    card.dueAt = now + card.intervalDays * 86_400;
  }
  return {
    dueAt: card.dueAt,
    intervalDays: card.intervalDays,
    repetitions: 0,
    lapses: rating === "again" ? 1 : 0,
    totalReviews: card.totalReviews,
    version: card.version,
    schedulerPhase: card.schedulerPhase,
    stability: card.intervalDays > 0 ? card.intervalDays : null,
    difficulty: 5,
  };
}

/**
 * 判定问答卡回答是否覆盖评分要点（演示级简化）。
 *
 * 契约：返回 AiEvaluationResult，progress 可为 null（由调用方决定是否再调用调度）。
 * 说明：仅做关键词的朴素包含匹配，不调用任何真实 AI，也不做语义理解。
 */
export function evaluateAnswer(card: MockCard, answer: string): AiEvaluationResult {
  const normalized = normalizeForMatch(answer);
  const matched = card.rubric.filter((point) => normalized.includes(normalizeForMatch(point)));
  // 仅演示：阈值与长度判断都是拍脑袋的，只为让界面能出现“对/错”两种反馈。
  const isCorrect = matched.length >= Math.min(2, Math.max(1, card.rubric.length)) || (card.rubric.length === 0 && normalized.length >= 24);
  return {
    isCorrect,
    feedback: isCorrect
      ? "回答正确。关键要点都已覆盖，表述不需要与参考答案一致。"
      : `还不完整。回答里缺少这些要点：${card.rubric.filter((point) => !matched.includes(point)).join("、")}。`,
    missingPoints: card.rubric.filter((point) => !matched.includes(point)),
    suggestedAnswer: card.back,
    progress: null,
  };
}

/** 归一化文本用于朴素匹配：去首尾空白、转小写、把连续空白压成单个空格。 */
function normalizeForMatch(value: string): string {
  return value.trim().toLowerCase().split(/\s+/).join(" ");
}

/** 将卡片摘要成复习队列条目，供复习界面消费。 */
export function toReviewCard(card: MockCard): ReviewCard {
  return {
    id: card.id,
    notePath: card.notePath,
    sourceRef: card.sourceRef,
    kind: card.kind,
    front: card.front,
    back: card.back,
    detail: card.detail,
    example: card.example,
    aliases: card.aliases,
    rubricPoints: card.rubric,
    state: card.schedulerPhase,
    version: card.version,
  };
}

/** 统计到期卡片数量（演示级）。 */
export function studyStats(now: number): StudyStats {
  let dueCount = 0;
  for (const card of cards.values()) {
    if (card.dueAt <= now) {
      dueCount += 1;
    }
  }
  return {
    dueCount,
    totalCards: cards.size,
    weeklyCompletionRate: null,
    weeklyCompletedCount: 0,
  };
}
