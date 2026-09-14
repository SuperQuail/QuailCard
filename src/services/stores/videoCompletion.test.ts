import { afterEach, beforeEach, expect, test, vi } from "vitest";
import * as api from "../../api/video";
import * as backend from "../../api/backend";
import * as noteStore from "./noteStore";
import { vaultPath } from "./vaultStore";
import { retryTask, startTask, videoState } from "./videoStore";
import type { VideoTaskStatus } from "../../domain/video";

vi.mock("./providerStore", () => ({ activeProviderId: { value: "provider" } }));
vi.mock("./vaultStore", () => ({ vaultPath: { value: null } }));
vi.mock("./noteStore", () => ({ refreshNotes: vi.fn(), selectNote: vi.fn(), updateNoteDraft: vi.fn() }));
vi.mock("../../api/backend", () => ({ syncNoteIndex: vi.fn() }));
vi.mock("../../api/video", () => ({ start: vi.fn(), status: vi.fn(), history: vi.fn() }));

const running: VideoTaskStatus = { taskId: "a", state: "running", step: "下载", progress: 1,
  message: "", sequence: 2, segments: 0, shots: 0, transcriptSource: "", notePath: null, error: null };
const completed: VideoTaskStatus = { ...running, state: "completed", sequence: 3, notePath: "video.md" };

/** 可控请求验证启动与刷新等待期间切换 Vault 的边界。 */
function deferred<T>() {
  let resolve!: (value: T) => void;
  const promise = new Promise<T>(yes => { resolve = yes; });
  return { promise, resolve };
}

// 隔离真实控制器的定时器，并提供通过表单校验的最小任务参数。
beforeEach(() => {
  vi.useFakeTimers();
  vi.resetAllMocks();
  vaultPath.value = "origin";
  videoState.task = null;
  videoState.starting = false;
  videoState.url = "BV1234567890";
  videoState.probeUrl = videoState.url;
  videoState.probe = { bvid: videoState.url, title: "video", owner: "owner", duration: 1, loggedIn: false,
    pages: [{ page: 1, title: "one", duration: 1 }],
    qualities: [{ qn: 32, height: 480, label: "480P", available: true, requiresVip: false, estimatedBytes: 1 }] };
  videoState.quality = 32;
  videoState.pages = [1];
  vi.mocked(api.start).mockResolvedValue(running);
  vi.mocked(api.status).mockResolvedValue(completed);
  vi.mocked(api.history).mockResolvedValue([]);
  vi.mocked(backend.syncNoteIndex).mockResolvedValue(1);
  vi.mocked(noteStore.refreshNotes).mockResolvedValue(undefined);
});
// 不允许测试留下轮询，也不允许自动完成去碰编辑器。
afterEach(() => {
  expect(noteStore.selectNote).not.toHaveBeenCalled();
  expect(noteStore.updateNoteDraft).not.toHaveBeenCalled();
  vi.clearAllTimers();
  vi.useRealTimers();
});

// 侧栏摘要必须在索引可用后刷新，启动即完成与轮询完成使用相同契约。
test.each([false, true])("同库生成完成按序刷新一次，即时完成=%s", async immediate => {
  if (immediate) vi.mocked(api.start).mockResolvedValue(completed);
  vi.mocked(noteStore.refreshNotes).mockImplementation(async () => {
    expect(backend.syncNoteIndex).toHaveBeenCalledExactlyOnceWith("video.md");
  });
  await startTask();
  await vi.advanceTimersByTimeAsync(6000);
  expect(noteStore.refreshNotes).toHaveBeenCalledTimes(1);
  expect(videoState.task?.state).toBe("completed");
});

// 捕获必须发生在 api.start 返回之前，不能把启动时的归属替换为新库。
test("启动请求等待期间切库跳过刷新，重试重新捕获新库", async () => {
  const starting = deferred<VideoTaskStatus>();
  vi.mocked(api.start).mockReturnValueOnce(starting.promise);
  const work = startTask();
  // 等到请求实际发出；此前仍可能在等待上一轮刷新，尚未确定新任务归属。
  await vi.advanceTimersByTimeAsync(0);
  expect(api.start).toHaveBeenCalledTimes(1);
  vaultPath.value = "other";
  starting.resolve(completed);
  await work;
  expect(backend.syncNoteIndex).not.toHaveBeenCalled();
  expect(noteStore.refreshNotes).not.toHaveBeenCalled();
  vi.mocked(api.start).mockResolvedValueOnce({ ...completed, taskId: "retry" });
  await retryTask();
  expect(backend.syncNoteIndex).toHaveBeenCalledExactlyOnceWith("video.md");
  expect(noteStore.refreshNotes).toHaveBeenCalledTimes(1);
});

// 已经开始建索引后离开原库也不能刷新新库侧栏。
test("索引请求期间切库跳过列表刷新", async () => {
  const syncing = deferred<number>();
  vi.mocked(api.start).mockResolvedValue(completed);
  vi.mocked(backend.syncNoteIndex).mockReturnValueOnce(syncing.promise);
  const work = startTask();
  await vi.advanceTimersByTimeAsync(0);
  expect(backend.syncNoteIndex).toHaveBeenCalledTimes(1);
  vaultPath.value = "other";
  syncing.resolve(1);
  await work;
  expect(noteStore.refreshNotes).not.toHaveBeenCalled();
});

// 两种刷新错误仅供手动打开时补救，不允许把成功生成回滚成失败。
test.each(["index", "list"])("刷新失败保留成功任务：%s", async phase => {
  if (phase === "index") vi.mocked(backend.syncNoteIndex).mockRejectedValueOnce(new Error("refresh failed"));
  else vi.mocked(noteStore.refreshNotes).mockRejectedValueOnce(new Error("refresh failed"));
  await startTask();
  await vi.advanceTimersByTimeAsync(6000);
  expect(videoState.task).toEqual(completed);
  expect(videoState.taskError).toBe("refresh failed");
  expect(videoState.error).toBe("");
  expect(api.start).toHaveBeenCalledTimes(1);
  expect(backend.syncNoteIndex).toHaveBeenCalledTimes(1);
});
