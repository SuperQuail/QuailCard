import { beforeEach, describe, expect, test, vi } from "vitest";
import { createReviewSession, type ReviewSnapshot } from "./reviewSession";
import * as backend from "../api/backend";
import { getReviewQueue } from "./stores/reviewStore";
import type { ReviewCard } from "../domain/types";
vi.mock("../api/backend", () => ({ submitReview: vi.fn(), evaluateAnswer: vi.fn() }));
vi.mock("./stores/reviewStore", () => ({ getReviewQueue: vi.fn(), refreshStats: vi.fn().mockResolvedValue(undefined) }));
const card: ReviewCard = { id: "c1", notePath: "a.md", sourceRef: "", kind: "qa", front: "为什么？", back: "原理", detail: "", example: "", aliases: [], rubricPoints: [], state: "new", version: 1 };
beforeEach(() => { vi.clearAllMocks(); vi.mocked(getReviewQueue).mockResolvedValue([{ ...card }]); vi.mocked(backend.submitReview).mockResolvedValue({} as never); });
describe("共享复习流程", () => {
  test("读取和揭示不写调度，同次评分重试复用原身份及评分", async () => {
    const flow = createReviewSession({ id: "session", paths: [], includeAll: false });
    await flow.loadQueue(); await flow.nextCard();
    expect(backend.submitReview).not.toHaveBeenCalled();
    vi.mocked(backend.submitReview).mockRejectedValueOnce(new Error("网络中断"));
    await flow.rate("hard"); expect(flow.finished.value).toBe(false);
    await flow.rate("good");
    expect(vi.mocked(backend.submitReview).mock.calls).toEqual([["c1", "hard", 1, "session:0:c1:1"], ["c1", "hard", 1, "session:0:c1:1"]]);
    expect(flow.stats.hard).toBe(1); expect(flow.stats.good).toBe(0); expect(flow.finished.value).toBe(true);
    await flow.rate("good"); expect(backend.submitReview).toHaveBeenCalledTimes(2);
  });
  test("已提交但尚未翻页的历史恢复时跳过已完成卡片", async () => {
    const saved: ReviewSnapshot = { queue: [card], index: 0, finished: false, stats: { again: 0, hard: 0, good: 1 }, completedIds: [card.id] };
    const flow = createReviewSession({ id: "old", paths: [], includeAll: false, saved, persist: vi.fn().mockResolvedValue(undefined) });
    await flow.loadQueue();
    expect(flow.finished.value).toBe(true); expect(getReviewQueue).not.toHaveBeenCalled(); expect(backend.submitReview).not.toHaveBeenCalled();
  });
  test("多篇笔记队列去重，再来一轮重新获取版本", async () => {
    const flow = createReviewSession({ id: "multi", paths: ["a.md", "b.md"], includeAll: true });
    await flow.loadQueue(); expect(flow.queue.value).toHaveLength(1);
    expect(getReviewQueue).toHaveBeenCalledWith("a.md", true); expect(getReviewQueue).toHaveBeenCalledWith("b.md", true);
    await flow.rate("good");
    vi.mocked(getReviewQueue).mockResolvedValue([{ ...card, version: 2 }]); await flow.restartRound(); await flow.rate("good");
    expect(backend.submitReview).toHaveBeenLastCalledWith("c1", "good", 2, "multi:1:c1:2");
  });
  test("AI 判定期间持有忙状态，重试保留最初作答并且只累计一次", async () => {
    const flow = createReviewSession({ id: "ai", paths: [], includeAll: false }); await flow.loadQueue();
    vi.mocked(backend.evaluateAnswer).mockRejectedValueOnce(new Error("网络错误")).mockResolvedValue({ isCorrect: false, feedback: "缺少要点", suggestedAnswer: "原理", missingPoints: [], progress: null });
    await expect(flow.evaluate(card.id, "第一次回答", 1)).rejects.toThrow("网络错误"); expect(flow.busy.value).toBe(false);
    await flow.evaluate(card.id, "改写回答", 1);
    expect(backend.evaluateAnswer).toHaveBeenLastCalledWith("c1", "第一次回答", 1, "ai:0:c1:1");
    expect(flow.stats.again).toBe(1); expect(flow.outcomes.value[0].answer).toBe("第一次回答");
  });
});
