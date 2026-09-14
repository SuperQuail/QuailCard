import { afterEach, expect, test, vi } from "vitest";
import * as api from "../../api/video";
import type { VideoStartInput, VideoTaskStatus } from "../../domain/video";
import { createVideoTasks } from "./videoTasks";
vi.mock("../../api/video", () => ({ start: vi.fn(), status: vi.fn(), cancel: vi.fn() }));

/** 每例使用独立控制器，定时器不会跨任务用例泄漏。 */
function fixture(captureCompletion?: () => (notePath: string) => void | Promise<void>) {
  vi.useFakeTimers();
  const task: VideoTaskStatus = { taskId: "a", state: "running", step: "下载", progress: 1,
    message: "", sequence: 2, segments: 0, shots: 0, transcriptSource: "", notePath: null, error: null };
  const state = { task: null as VideoTaskStatus | null, starting: false, error: "", taskError: "" };
  const input: VideoStartInput = { url: "BV1234567890", providerId: "original", quality: 32, pages: [2], screenshots: false, mode: "note" };
  vi.mocked(api.start).mockResolvedValue(task);
  return { task, state, input, controller: createVideoTasks(state, captureCompletion) };
}
/** 可控请求模拟慢网络，无真实等待。 */
function deferred<T>() {
  let resolve!: (value: T) => void;
  const promise = new Promise<T>(yes => { resolve = yes; });
  return { promise, resolve };
}
// 每例释放全部计时器与命令桩。
afterEach(() => { vi.clearAllTimers(); vi.useRealTimers(); vi.resetAllMocks(); });

// 只有一个启动命令，重试复制原始参数而不读取可编辑引用。
test("重复开始被拦截，重试保留原始任务输入", async () => {
  const { task, state, input, controller } = fixture();
  const pending = deferred<VideoTaskStatus>();
  vi.mocked(api.start).mockReturnValueOnce(pending.promise);
  const starting = controller.start(input);
  await controller.start({ ...input, url: "other" });
  pending.resolve(task);
  await starting;
  expect(api.start).toHaveBeenCalledTimes(1);
  input.pages.push(3);
  input.providerId = "changed";
  state.task = { ...task, state: "failed" };
  await controller.retry();
  expect(api.start).toHaveBeenLastCalledWith({ ...input, pages: [2], providerId: "original" });
});

// 请求完成后重新计时；错任务、倒退序号与临时错误都不能破坏可信快照。
test("轮询串行并在临时错误后恢复，拒绝错任务和旧序号", async () => {
  const { task, state, input, controller } = fixture();
  const pending = deferred<VideoTaskStatus>();
  vi.mocked(api.status).mockReturnValueOnce(pending.promise)
    .mockRejectedValueOnce(new Error("temporary"))
    .mockResolvedValueOnce({ ...task, taskId: "wrong", sequence: 100 })
    .mockResolvedValueOnce({ ...task, sequence: 1 })
    .mockResolvedValueOnce({ ...task, sequence: 3, state: "completed" });
  await controller.start(input);
  await vi.advanceTimersByTimeAsync(15000);
  expect(api.status).toHaveBeenCalledTimes(1);
  pending.resolve({ ...task, progress: 20 });
  await vi.advanceTimersByTimeAsync(0);
  await vi.advanceTimersByTimeAsync(1500);
  expect(state.taskError).toBe("temporary");
  expect(state.task?.progress).toBe(20);
  await vi.advanceTimersByTimeAsync(3000);
  expect(state.task?.sequence).toBe(2);
  expect(state.task?.taskId).toBe("a");
  await vi.advanceTimersByTimeAsync(1500);
  expect(state.task?.state).toBe("completed");
  expect(state.taskError).toBe("");
  await vi.advanceTimersByTimeAsync(6000);
  expect(api.status).toHaveBeenCalledTimes(5);
});

// 只有已接受且带笔记路径的成功终态触发一次副作用。
test.each([false, true])("完成钩子只执行一次，即时完成=%s", async immediate => {
  const complete = vi.fn();
  const capture = vi.fn(() => complete);
  const { task, input, controller } = fixture(capture);
  const done = { ...task, state: "completed" as const, sequence: 3, notePath: "video.md" };
  vi.mocked(api.start).mockImplementation(async () => {
    expect(capture).toHaveBeenCalledTimes(1);
    return immediate ? done : task;
  });
  vi.mocked(api.status).mockResolvedValue(done);
  await controller.start(input);
  await controller.poll();
  await controller.poll();
  await vi.advanceTimersByTimeAsync(6000);
  expect(complete).toHaveBeenCalledExactlyOnceWith("video.md");
});

// 刷新失败留在任务提示，不污染生成成功，也不自动重试生成或回调。
test.each([false, true])("完成副作用失败隔离，即时完成=%s", async immediate => {
  const complete = vi.fn().mockRejectedValue(new Error("refresh failed"));
  const { task, state, input, controller } = fixture(() => complete);
  const done = { ...task, state: "completed" as const, sequence: 3, notePath: "video.md" };
  vi.mocked(api.start).mockResolvedValue(immediate ? done : task);
  vi.mocked(api.status).mockResolvedValue(done);
  await controller.start(input);
  await controller.poll();
  await vi.advanceTimersByTimeAsync(6000);
  expect(state.task).toEqual(done);
  expect(state.taskError).toBe("refresh failed");
  expect(state.error).toBe("");
  expect(complete).toHaveBeenCalledTimes(1);
  expect(api.start).toHaveBeenCalledTimes(1);
});

// 错任务与倒退序号即使声称成功也没有完成权限。
test("拒绝陈旧完成响应，失败取消和缺路径不回调", async () => {
  const complete = vi.fn();
  const { task, input, controller } = fixture(() => complete);
  vi.mocked(api.status)
    .mockResolvedValueOnce({ ...task, taskId: "wrong", sequence: 100, state: "completed", notePath: "wrong.md" })
    .mockResolvedValueOnce({ ...task, sequence: 1, state: "completed", notePath: "old.md" })
    .mockResolvedValueOnce({ ...task, sequence: 3, state: "failed", notePath: "failed.md" });
  await controller.start(input);
  await controller.poll();
  await controller.poll();
  await controller.poll();
  vi.mocked(api.start).mockResolvedValueOnce({ ...task, state: "cancelled", notePath: "cancelled.md" });
  await controller.retry();
  vi.mocked(api.start).mockResolvedValueOnce({ ...task, state: "completed" });
  await controller.retry();
  expect(complete).not.toHaveBeenCalled();
});

// 旧请求跨过重试代次后不可把完成事件发给新任务的捕获范围。
test("重试拒绝前代在途完成响应并重新捕获范围", async () => {
  const oldComplete = vi.fn();
  const newComplete = vi.fn();
  const capture = vi.fn().mockReturnValueOnce(oldComplete).mockReturnValueOnce(newComplete);
  const { task, state, input, controller } = fixture(capture);
  const pending = deferred<VideoTaskStatus>();
  await controller.start(input);
  vi.mocked(api.status).mockReturnValueOnce(pending.promise);
  const polling = controller.poll();
  state.task = { ...task, state: "failed" };
  vi.mocked(api.start).mockResolvedValueOnce({ ...task, taskId: "b", state: "completed", notePath: "new.md" });
  const retrying = controller.retry();
  pending.resolve({ ...task, state: "completed", sequence: 3, notePath: "old.md" });
  await polling;
  await retrying;
  expect(capture).toHaveBeenCalledTimes(2);
  expect(oldComplete).not.toHaveBeenCalled();
  expect(newComplete).toHaveBeenCalledExactlyOnceWith("new.md");
});

// 取消查询复用正在飞行的状态请求，不与定时器争抢写权限。
test("停止时复用在途查询", async () => {
  const { task, input, controller } = fixture();
  const pending = deferred<VideoTaskStatus>();
  vi.mocked(api.status).mockReturnValue(pending.promise);
  vi.mocked(api.cancel).mockResolvedValue(undefined);
  await controller.start(input);
  await vi.advanceTimersByTimeAsync(1500);
  const stop = controller.stop();
  await vi.advanceTimersByTimeAsync(10000);
  expect(api.status).toHaveBeenCalledTimes(1);
  pending.resolve({ ...task, sequence: 3, state: "cancelled" });
  await stop;
  expect(api.cancel).toHaveBeenCalledWith("a");
});
