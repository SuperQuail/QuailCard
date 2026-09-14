import { afterEach, expect, test, vi } from "vitest";
import * as api from "../../api/video";
import type { VideoDownloadStatus, VideoModelStatus, VideoSettings } from "../../domain/video";
import { createVideoResources } from "./videoResources";
vi.mock("../../api/video", () => ({ saveSettings: vi.fn(), downloadModel: vi.fn(), downloadStatus: vi.fn(),
  cancelModel: vi.fn(), components: vi.fn(), models: vi.fn(), getSettings: vi.fn() }));

/** 最小设置仅测试字段合并契约，不依赖后端默认配置。 */
function fixture() {
  const state = { settings: { noteFolder: "old", asrModel: "base" } as VideoSettings,
    components: [], models: [] as VideoModelStatus[], downloadingModel: "", downloadStatus: null as VideoDownloadStatus | null, error: "" };
  return { state, controller: createVideoResources(state) };
}
// 独立释放定时资源与模拟调用。
afterEach(() => { vi.clearAllTimers(); vi.useRealTimers(); vi.resetAllMocks(); });

// 第二个补丁必须基于第一条已确认的返回值，而非最初的旧对象。
test("串行补丁不丢失字段，失败后队列仍可继续", async () => {
  const { state, controller } = fixture();
  let complete!: (settings: VideoSettings) => void;
  vi.mocked(api.saveSettings).mockImplementationOnce(() => new Promise(yes => { complete = yes; }))
    .mockImplementationOnce(async settings => settings)
    .mockRejectedValueOnce(new Error("write failed"))
    .mockImplementationOnce(async settings => settings);
  const first = controller.save({ noteFolder: "new" });
  const second = controller.save({ asrModel: "small" });
  await Promise.resolve();
  expect(api.saveSettings).toHaveBeenCalledTimes(1);
  complete({ ...state.settings, noteFolder: "new" });
  await Promise.all([first, second]);
  expect(state.settings.noteFolder).toBe("new");
  expect(state.settings.asrModel).toBe("small");
  await controller.save({ noteFolder: "failed" });
  await controller.save({ asrModel: "medium" });
  expect(state.settings.noteFolder).toBe("new");
  expect(state.settings.asrModel).toBe("medium");
});

// 取消信号不代表下载已退出，互斥必须一直持续到阻塞命令结束。
test("模型取消不提前释放下载锁", async () => {
  vi.useFakeTimers();
  const { state, controller } = fixture();
  let complete!: (value: VideoModelStatus) => void;
  vi.mocked(api.downloadModel).mockImplementationOnce(() => new Promise(yes => { complete = yes; }));
  vi.mocked(api.downloadStatus).mockRejectedValue(new Error("temporary"));
  vi.mocked(api.cancelModel).mockResolvedValue(undefined);
  vi.mocked(api.components).mockResolvedValue([]);
  vi.mocked(api.models).mockResolvedValue([]);
  const downloading = controller.download("base");
  await controller.cancel();
  await controller.download("small");
  expect(state.downloadingModel).toBe("base");
  expect(api.downloadModel).toHaveBeenCalledTimes(1);
  expect(api.cancelModel).toHaveBeenCalledTimes(1);
  complete({ id: "base", label: "Base", bytes: 1, status: "ready" });
  await downloading;
  expect(state.downloadingModel).toBe("");
  expect(api.models).toHaveBeenCalledTimes(1);
});

// 页面刷新不应遗失后端仍在运行的下载，接管轮询能处理临时错误和终态。
test("恢复后端下载并在取消终态重新检测模型", async () => {
  vi.useFakeTimers();
  const { state, controller } = fixture();
  const active: VideoDownloadStatus = { modelId: "base", state: "downloading", downloadedBytes: 10,
    totalBytes: 100, bytesPerSecond: 1, error: null };
  vi.mocked(api.getSettings).mockResolvedValue(state.settings);
  vi.mocked(api.components).mockResolvedValue([]);
  vi.mocked(api.models).mockResolvedValue([{ id: "base", label: "Base", bytes: 1, status: "ready" }]);
  vi.mocked(api.downloadStatus).mockResolvedValueOnce(active)
    .mockRejectedValueOnce(new Error("temporary"))
    .mockResolvedValueOnce({ ...active, state: "cancelled" });
  await controller.load();
  expect(state.downloadingModel).toBe("base");
  await controller.refresh();
  expect(api.downloadStatus).toHaveBeenCalledTimes(1);
  await vi.advanceTimersByTimeAsync(1000);
  expect(state.downloadingModel).toBe("base");
  await vi.advanceTimersByTimeAsync(1000);
  expect(state.downloadingModel).toBe("");
  expect(state.downloadStatus?.state).toBe("cancelled");
  expect(state.models[0]?.status).toBe("ready");
  expect(api.models).toHaveBeenCalledTimes(3);
  await vi.advanceTimersByTimeAsync(5000);
  expect(api.downloadStatus).toHaveBeenCalledTimes(3);
});

// 失败也必须重新检测，已有的有效模型不能被下载错误当作缺失。
test("下载失败保留真实已安装模型并读取终态", async () => {
  const { state, controller } = fixture();
  vi.mocked(api.downloadModel).mockRejectedValue(new Error("download failed"));
  vi.mocked(api.components).mockResolvedValue([]);
  vi.mocked(api.models).mockResolvedValue([{ id: "base", label: "Base", bytes: 1, status: "ready" }]);
  vi.mocked(api.downloadStatus).mockResolvedValue({ modelId: "base", state: "failed",
    downloadedBytes: 10, totalBytes: 100, bytesPerSecond: 0, error: "download failed" });
  await controller.download("base");
  expect(state.models[0]?.status).toBe("ready");
  expect(state.downloadStatus?.state).toBe("failed");
  expect(state.downloadingModel).toBe("");
});
