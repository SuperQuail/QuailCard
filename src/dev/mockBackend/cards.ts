import type { CardInput, DictationResult, GenerationResult, NoteCard, ReviewCard } from "../../domain/types";
import { advanceCard, evaluateAnswer, studyStats, toReviewCard } from "./scheduling";
import { cardSequence, cards, makeCard, setCardSequence } from "./state";
import { toNoteCard } from "./notes";

/**
 * 卡片域演示处理：卡片增删改查、复习队列、听写判定、回答判定与拆卡草稿。
 *
 * 说明：全部为内存演示。调度与判定逻辑见 scheduling.ts（演示级简化）。
 * 不实现真实复习调度或真实 AI 拆卡，仅保证 UI 数据形状正确。
 */

/** 保存或更新单张卡片。 */
export function saveCard(input: CardInput): NoteCard {
  const existing = input.id ? cards.get(input.id) : undefined;
  if (existing) {
    Object.assign(existing, {
      sourceRef: input.sourceRef ?? existing.sourceRef,
      kind: input.kind === "vocabulary" ? "vocabulary" : "qa",
      front: input.front,
      back: input.back,
      detail: input.detail ?? existing.detail,
      example: input.example ?? existing.example,
      aliases: input.aliases ?? existing.aliases,
      rubric: input.rubric ?? existing.rubric,
    });
    return toNoteCard(existing);
  }
  // 新建：仅演示，用单调递增的序号保证排序稳定。
  const nextSequence = cardSequence + 1;
  const card = makeCard(input.notePath, input.kind === "vocabulary" ? "vocabulary" : "qa", input.front, input.back, input.detail ?? "", input.example ?? "", nextSequence);
  card.sourceRef = input.sourceRef ?? "";
  card.aliases = input.aliases ?? [];
  card.rubric = input.rubric ?? [];
  cards.set(card.id, card);
  setCardSequence(nextSequence);
  return toNoteCard(card);
}

/** 删除单张卡片。 */
export function deleteCard(cardId: string): void {
  cards.delete(cardId);
}

/** 查询指定笔记的卡片列表，按 position 升序。 */
export function listNoteCards(notePath: string): NoteCard[] {
  return [...cards.values()]
    .filter((card) => card.notePath === notePath)
    .sort((a, b) => a.position - b.position)
    .map(toNoteCard);
}

/** 采纳拆卡草稿并落库为卡片，返回新增数量。 */
export function adoptCards(input: { notePath: string; kind: string; cards: Array<{ fields: Record<string, string> }> }): number {
  let count = 0;
  for (const draft of input.cards) {
    const fields = draft.fields;
    if (!fields.front?.trim() || !fields.back?.trim()) {
      continue;
    }
    const nextSequence = cardSequence + 1;
    const card = makeCard(input.notePath, input.kind as "vocabulary" | "qa" | "ai", fields.front, fields.back, fields.detail ?? "", fields.example ?? "", nextSequence);
    card.sourceRef = fields.source ?? "";
    card.rubric = (fields.rubric ?? "").split(/[、,，]/).map((item) => item.trim()).filter(Boolean);
    cards.set(card.id, card);
    setCardSequence(nextSequence);
    count += 1;
  }
  return count;
}

/** 读取复习队列，可按笔记过滤，可强制包含未到期卡片。 */
export function getReviewQueue(notePath: string | null, includeAll: boolean, now: number): ReviewCard[] {
  return [...cards.values()]
    .filter((card) => (notePath ? card.notePath === notePath : true) && (includeAll || card.dueAt <= now))
    .sort((left, right) => left.dueAt - right.dueAt || left.position - right.position)
    .map(toReviewCard);
}

/** 归一化听写答案：去首尾空白、转小写、压缩空白。 */
export function normalizeAnswer(value: string): string {
  return value.trim().toLowerCase().split(/\s+/).join(" ");
}

/** 听写判定（演示级：与答案/别名做精确归一化匹配）。 */
export function checkDictation(cardId: string, answer: string): DictationResult {
  const card = cards.get(cardId);
  if (!card || card.kind !== "vocabulary") {
    throw new Error("单词卡不存在");
  }
  const normalized = normalizeAnswer(answer);
  const candidates = [card.back, ...card.aliases].map(normalizeAnswer);
  return {
    correct: normalized.length > 0 && candidates.includes(normalized),
    expected: card.back,
    aliases: card.aliases,
  };
}

/** 提交复习评分并推进调度（演示级）。 */
export function submitReview(cardId: string, rating: string): ReturnType<typeof advanceCard> {
  const card = cards.get(cardId);
  if (!card) {
    throw new Error("卡片不存在");
  }
  return advanceCard(card, rating);
}

/** 判定回答并推进调度（演示级）。 */
export function evaluateAnswerCommand(cardId: string, answer: string): ReturnType<typeof evaluateAnswer> & { progress: ReturnType<typeof advanceCard> } {
  const card = cards.get(cardId);
  if (!card) {
    throw new Error("卡片不存在");
  }
  const result = evaluateAnswer(card, answer);
  return { ...result, progress: advanceCard(card, result.isCorrect ? "good" : "again") };
}

/** 学习统计（演示级）。 */
export function getStudyStats(now: number) {
  return studyStats(now);
}

/** 拆卡草稿（演示级：返回写死的示例，不调用真实 AI）。 */
export function generateCards(input: { typeId: string; requestedCount: number }): GenerationResult {
  const kind = input.typeId === "vocabulary" ? "vocabulary" : input.typeId === "ai" ? "ai" : "qa";
  const drafts = demoDrafts(kind);
  const count = input.requestedCount > 0 ? Math.min(input.requestedCount, drafts.length) : drafts.length;
  return { cards: drafts.slice(0, count), warnings: [] };
}

/** 按类型生成演示拆卡草稿。 */
function demoDrafts(kind: string): Array<{ fields: Record<string, string> }> {
  if (kind === "vocabulary") {
    return [
      { fields: { front: "无处不在的；普遍存在的", back: "ubiquitous", detail: "/juːˈbɪkwɪtəs/", example: "Mobile devices have become ubiquitous in modern life.", rubric: "", source: "人类大脑对图像的记忆远强于抽象文字。" } },
      { fields: { front: "一丝不苟的；极其仔细的", back: "meticulous", detail: "/məˈtɪkjələs/", example: "The researcher kept meticulous records of every trial.", rubric: "", source: "刻意练习要求持续停留在学习区。" } },
      { fields: { front: "减轻；缓解", back: "alleviate", detail: "/əˈliːvieɪt/", example: "The policy may alleviate pressure on local hospitals.", rubric: "", source: "把知识画成结构图比做线性笔记更利于回忆。" } },
    ];
  }
  if (kind === "ai") {
    return [
      { fields: { front: "什么是费曼学习法？请用自己的话说明它的核心观点。", back: "费曼学习法要求用最简单的语言把一个概念讲给完全不懂的人听；讲不清楚，说明自己还没有真正理解。", detail: "来源：灵感随手记", example: "", rubric: "简单语言复述、讲不清楚 = 没理解、以教促学", source: "如果你不能把一个概念讲给一个完全不懂的人听，说明你自己还没有真正理解它。" } },
      { fields: { front: "刻意练习的两个必要条件是什么？", back: "任务难度略高于当前能力（学习区），并且能够获得即时反馈。", detail: "来源：灵感随手记", example: "", rubric: "难度略高于能力、即时反馈", source: "任务难度略高于当前能力，同时能获得即时反馈。" } },
    ];
  }
  return [
    { fields: { front: "费曼学习法的核心观点是什么？", back: "如果你不能把一个概念讲给一个完全不懂的人听，说明你自己还没有真正理解它。", detail: "来源：灵感随手记", example: "", rubric: "", source: "如果你不能把一个概念讲给一个完全不懂的人听，说明你自己还没有真正理解它。" } },
    { fields: { front: "为什么结构图比线性笔记更利于回忆？", back: "人类大脑对图像的记忆远强于抽象文字，结构图把知识组织成空间关系，回忆时有更多提取线索。", detail: "来源：灵感随手记", example: "", rubric: "", source: "人类大脑对图像的记忆远强于抽象文字。" } },
  ];
}
