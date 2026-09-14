import { afterEach, beforeEach, describe, expect, test, vi } from "vitest";
import type { GeneratedCard, GenerationTaskStatus } from "../domain/types";
import { createAiSplitService } from "./aiSplitService";
import type { AiSplitSnapshot } from "./aiSplitTypes";

const snapshot: AiSplitSnapshot = { vaultPath: "V", notePath: "笔记.md", noteTitle: "笔记", noteContent: "原文", kind: "qa", selection: null };
const drafts: GeneratedCard[] = ["a", "b", "c"].map((draftId) => ({
  draftId, fields: { front: draftId, back: "答案", aliases: "甲、乙", rubric: "完整要点, 不应拆解", detail: "/test/", example: "示例", source: "原文" },
  source: { from: 0, to: 2, excerpt: "原文", prefix: "", suffix: "" },
}));

/** 受控 Promise 用于确定启动、停止及提交的先后顺序。 */
function deferred<T>() {
  let resolve!: (value: T) => void;
  const promise = new Promise<T>((done) => { resolve = done; });
  return { promise, resolve };
}

/** 默认终态带完整字段，个别测试覆盖为运行中或失败。 */
function status(overrides: Partial<GenerationTaskStatus> = {}): GenerationTaskStatus {
  return { taskId: "task", state: "completed", phase: "validating", generatedCount: drafts.length, result: { cards: drafts, warnings: [] }, error: null, ...overrides };
}

/** 每个场景使用独立端口，避免真实网络和文件写入。 */
function setup() {
  const ports = {
    providerReady: vi.fn(() => true), verifySnapshot: vi.fn().mockResolvedValue("hash"),
    startGeneration: vi.fn().mockResolvedValue({ taskId: "task" }),
    getGenerationStatus: vi.fn().mockResolvedValue(status()), cancelGeneration: vi.fn().mockResolvedValue(status({ state: "cancelled" })),
    adoptCards: vi.fn().mockResolvedValue({ addedIds: ["a", "b", "c"], existingIds: [], duplicateIds: [] }),
    refresh: vi.fn().mockResolvedValue(undefined), showToast: vi.fn(),
  };
  const service = createAiSplitService(ports);
  service.open(snapshot);
  return { service, ports };
}

beforeEach(() => { vi.useFakeTimers(); });
afterEach(() => { vi.clearAllTimers(); vi.useRealTimers(); });

describe("智能拆卡任务生命周期", () => {
  test("按 500ms 查询真实阶段，终态停止查询并展示警告", async () => {
    const { service, ports } = setup();
    ports.getGenerationStatus.mockResolvedValueOnce(status({ state: "running", phase: "lookup", generatedCount: 1, result: null }));
    ports.getGenerationStatus.mockResolvedValueOnce(status({ result: { cards: drafts, warnings: ["材料只支持三张卡"] } }));
    await service.start();
    await vi.advanceTimersByTimeAsync(499);
    expect(ports.getGenerationStatus).not.toHaveBeenCalled();
    await vi.advanceTimersByTimeAsync(1);
    expect(service.state.value).toMatchObject({ step: "running", phase: "lookup", generatedCount: 1 });
    await vi.advanceTimersByTimeAsync(500);
    expect(service.state.value).toMatchObject({ step: "drafts", warnings: ["材料只支持三张卡"] });
    await vi.advanceTimersByTimeAsync(5000);
    expect(ports.getGenerationStatus).toHaveBeenCalledTimes(2);
  });

  test("启动登记尚未返回时停止，拿到任务 ID 后补发取消并保留草稿", async () => {
    const { service, ports } = setup();
    const registration = deferred<{ taskId: string }>();
    ports.startGeneration.mockReturnValue(registration.promise);
    const starting = service.start();
    await Promise.resolve();
    await service.stop();
    registration.resolve({ taskId: "task" });
    await starting;
    expect(ports.cancelGeneration).toHaveBeenCalledExactlyOnceWith("task");
    expect(service.state.value.drafts).toEqual(drafts);
    expect(service.state.value.open).toBe(true);
  });

  test("保存期间停止不创建任务", async () => {
    const { service, ports } = setup();
    const save = deferred<string>();
    ports.verifySnapshot.mockReturnValue(save.promise);
    const starting = service.start();
    await service.stop();
    save.resolve("hash");
    await starting;
    expect(ports.startGeneration).not.toHaveBeenCalled();
    expect(service.state.value.step).toBe("drafts");
  });

  test("关闭后启动迟到响应被取消，不污染新会话", async () => {
    const { service, ports } = setup();
    const registration = deferred<{ taskId: string }>();
    ports.startGeneration.mockReturnValueOnce(registration.promise);
    const starting = service.start();
    await Promise.resolve();
    service.close();
    service.open({ ...snapshot, notePath: "另一篇.md" });
    registration.resolve({ taskId: "old" });
    await starting;
    expect(ports.cancelGeneration).toHaveBeenCalledWith("old");
    expect(service.state.value).toMatchObject({ step: "scope", drafts: [], snapshot: { notePath: "另一篇.md" } });
  });

  test("旧查询迟到不会取消新会话轮询，也不能覆盖停止结果", async () => {
    const { service, ports } = setup();
    const query = deferred<GenerationTaskStatus>();
    ports.getGenerationStatus.mockReturnValueOnce(query.promise);
    await service.start();
    await vi.advanceTimersByTimeAsync(500);
    service.close();
    service.open(snapshot);
    await service.start();
    query.resolve(status({ state: "running", result: null }));
    await Promise.resolve();
    await vi.advanceTimersByTimeAsync(500);
    expect(service.state.value.step).toBe("drafts");
    expect(ports.getGenerationStatus).toHaveBeenCalledTimes(2);
  });

  test("任务过期终止查询，临时查询失败仍等待有效草稿", async () => {
    const { service, ports } = setup();
    ports.getGenerationStatus.mockRejectedValueOnce(new Error("暂时离线"));
    await service.start();
    await vi.advanceTimersByTimeAsync(500);
    expect(service.state.value.step).toBe("running");
    expect(ports.cancelGeneration).not.toHaveBeenCalled();
    ports.getGenerationStatus.mockRejectedValueOnce({ code: "TASK_NOT_FOUND", message: "任务已过期" });
    await vi.advanceTimersByTimeAsync(5000);
    expect(service.state.value.step).toBe("scope");
    expect(ports.getGenerationStatus).toHaveBeenCalledTimes(2);
  });

  test("失败重试在新任务登记前停止不会取消旧任务", async () => {
    const { service, ports } = setup();
    ports.getGenerationStatus.mockResolvedValueOnce(status({ state: "failed", result: null, error: { code: "AI", message: "生成失败" } }));
    await service.start();
    await vi.advanceTimersByTimeAsync(500);
    const registration = deferred<{ taskId: string }>();
    ports.startGeneration.mockReturnValueOnce(registration.promise);
    const retry = service.start();
    await Promise.resolve();
    await service.stop();
    expect(ports.cancelGeneration).not.toHaveBeenCalled();
    registration.resolve({ taskId: "new" });
    await retry;
    expect(ports.cancelGeneration).toHaveBeenCalledWith("new");
  });

  test("旧任务的取消响应晚于重试时，不会结束新任务", async () => {
    const { service, ports } = setup();
    const cancellation = deferred<GenerationTaskStatus>();
    ports.cancelGeneration.mockReturnValueOnce(cancellation.promise);
    ports.getGenerationStatus.mockResolvedValueOnce(status({ state: "failed", result: null }));
    await service.start();
    const stopping = service.stop();
    await vi.advanceTimersByTimeAsync(500);
    ports.startGeneration.mockResolvedValueOnce({ taskId: "retry" });
    await service.start();
    cancellation.resolve(status({ state: "cancelled" }));
    await stopping;
    expect(service.state.value).toMatchObject({ step: "running", taskId: "retry", drafts: [] });
    await vi.advanceTimersByTimeAsync(500);
    expect(service.state.value.step).toBe("drafts");
  });
});

describe("智能拆卡采纳", () => {
  test("取消勾选后删除其他草稿仍保持选择，零选择不提交", async () => {
    const { service, ports } = setup();
    await service.start();
    await vi.advanceTimersByTimeAsync(500);
    service.toggleAccepted("b");
    service.removeDraft("a");
    expect([...service.state.value.accepted]).toEqual(["c"]);
    service.toggleAccepted("c");
    await service.adopt();
    expect(ports.adoptCards).not.toHaveBeenCalled();
  });

  test("双击只提交一次，完整字段和来源按原草稿传递，刷新失败仍成功关闭", async () => {
    const { service, ports } = setup();
    await service.start();
    await vi.advanceTimersByTimeAsync(500);
    const commit = deferred<{ addedIds: string[]; existingIds: string[]; duplicateIds: string[] }>();
    ports.adoptCards.mockReturnValue(commit.promise);
    ports.refresh.mockRejectedValue(new Error("刷新中断"));
    const saving = service.adopt();
    await service.adopt();
    await Promise.resolve();
    expect(ports.adoptCards).toHaveBeenCalledExactlyOnceWith({ expectedVaultPath: "V", expectedNoteHash: "hash", notePath: "笔记.md", kind: "qa", cards: drafts });
    service.close();
    expect(service.state.value.open).toBe(true);
    commit.resolve({ addedIds: ["a"], existingIds: ["b"], duplicateIds: ["c"] });
    await saving;
    expect(service.state.value.open).toBe(false);
    expect(ports.showToast).toHaveBeenLastCalledWith(expect.stringContaining("卡片已保存，刷新失败"));
  });

  test("校验或写盘失败保留原草稿与勾选，允许使用相同 ID 重试", async () => {
    const { service, ports } = setup();
    await service.start();
    await vi.advanceTimersByTimeAsync(500);
    service.toggleAccepted("b");
    ports.adoptCards.mockRejectedValueOnce(new Error("写盘失败"));
    await service.adopt();
    expect(service.state.value).toMatchObject({ open: true, saving: false, drafts });
    expect([...service.state.value.accepted]).toEqual(["a", "c"]);
    ports.verifySnapshot.mockRejectedValueOnce(new Error("正文已变化"));
    await service.adopt();
    expect(ports.adoptCards).toHaveBeenCalledTimes(1);
    expect(ports.refresh).not.toHaveBeenCalled();
  });

  test("原笔记失效停止生成、保留草稿并拒绝采纳", async () => {
    const { service, ports } = setup();
    await service.start();
    service.invalidate("原笔记已改名");
    await Promise.resolve();
    await service.adopt();
    expect(service.state.value).toMatchObject({ drafts, invalidReason: "原笔记已改名" });
    expect(ports.adoptCards).not.toHaveBeenCalled();
  });
});
