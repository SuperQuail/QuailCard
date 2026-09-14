import { afterEach, beforeEach, expect, test, vi } from "vitest";
import * as api from "../../api/video";
import type { VideoProbe, VideoTaskStatus } from "../../domain/video";
import { probeLink, startTask, videoState, logout, restoreHistory } from "./videoStore";
import { chooseQuality, inputPage } from "./videoSelection";
vi.mock("./providerStore", () => ({ activeProviderId: { value: "provider" } }));
vi.mock("../../api/video", () => ({ probe: vi.fn(), start: vi.fn(), logout: vi.fn(), history: vi.fn() }));
const url = "https://www.bilibili.com/video/BV1234567890?p=2";

/** 提供有序无关的档位与多分 P 数据用于验证输入选择契约。 */
function fixture(): VideoProbe {
  return { bvid: "BV1234567890", title: "title", owner: "owner", duration: 100, loggedIn: false,
    pages: [{ page: 1, title: "one", duration: 100 }, { page: 2, title: "two", duration: 200 }],
    qualities: [80, 16, 32].map(qn => ({ qn, height: 0, label: String(qn), available: true,
      requiresVip: false, estimatedBytes: 1000 })) };
}
/** 可控请求覆盖编辑输入之后才返回的旧解析。 */
function deferred<T>() {
  let resolve!: (value: T) => void;
  let reject!: (error: unknown) => void;
  const promise = new Promise<T>((yes, no) => { resolve = yes; reject = no; });
  return { promise, resolve, reject };
}
// 每例清空共享状态，URL 通过同步侦听器撤销前一例解析。
beforeEach(() => {
  vi.resetAllMocks();
  videoState.url = "";
  videoState.probe = null;
  videoState.probeUrl = "";
  videoState.task = null;
  videoState.starting = false;
  videoState.settings = null;
  videoState.login = null;
  videoState.loginOpen = false;
  videoState.error = "";
  videoState.mode = "note";
  vi.mocked(api.probe).mockResolvedValue(fixture());
});
// 本文件不启动真实任务或持久化用户设置。
afterEach(() => { vi.clearAllMocks(); });

// 分享文本中的 p 由输入助手提取；后端可补充短链选择。
test("自动选择480P和输入P参数，不按接口首项选高清", async () => {
  videoState.url = url;
  await probeLink();
  expect(videoState.quality).toBe(32);
  expect(videoState.pages).toEqual([2]);
  expect(inputPage("分享 " + url)).toBe(2);
  expect(inputPage(url.replace("p=2", "p=-1"))).toBeUndefined();
  const probe = fixture();
  probe.qualities = probe.qualities.filter(item => item.qn !== 32);
  expect(chooseQuality(probe)).toBe(16);
  probe.qualities = probe.qualities.filter(item => item.qn !== 16);
  expect(chooseQuality(probe)).toBe(80);
});

// 登录刷新只撤销失效清晰度，截图和已选分 P 保持用户选择。
test("登录刷新保留有效选项并在旧档位失效时降档", async () => {
  videoState.url = url;
  await probeLink();
  videoState.quality = 80;
  videoState.pages = [1, 2];
  videoState.screenshots = false;
  await probeLink(true);
  expect(videoState.quality).toBe(80);
  expect(videoState.pages).toEqual([1, 2]);
  expect(videoState.screenshots).toBe(false);
  const probe = fixture();
  probe.qualities[0]!.available = false;
  vi.mocked(api.probe).mockResolvedValue(probe);
  await probeLink(true);
  expect(videoState.quality).toBe(32);
});

// 开始任务必须按 wire 契约提交当前输出模式，未选择时缺省为笔记模式。
test("开始任务提交输出模式，缺省为笔记", async () => {
  const task: VideoTaskStatus = { taskId: "started", state: "completed", step: "", progress: 100,
    message: "", sequence: 1, segments: 0, shots: 0, transcriptSource: "", notePath: null, error: null };
  vi.mocked(api.start).mockResolvedValue(task);
  videoState.url = url;
  await probeLink();
  await startTask();
  expect(api.start).toHaveBeenNthCalledWith(1, expect.objectContaining({ mode: "note" }));
  videoState.mode = "transcript";
  await startTask();
  expect(api.start).toHaveBeenNthCalledWith(2, expect.objectContaining({ mode: "transcript" }));
});

// 两种迟到结果都不能为新 URL 解锁启动按钮。
test.each([false, true])("编辑URL拒绝旧解析结果或异常，失败=%s", async failure => {
  const pending = deferred<VideoProbe>();
  vi.mocked(api.probe).mockReturnValueOnce(pending.promise);
  videoState.url = url;
  const probing = probeLink();
  videoState.url = "BV0987654321";
  expect(videoState.probe).toBeNull();
  expect(videoState.probing).toBe(false);
  if (failure) pending.reject(new Error("stale"));
  else pending.resolve(fixture());
  await probing;
  expect(videoState.probe).toBeNull();
  expect(videoState.error).toBe("");
  await startTask();
  expect(api.start).not.toHaveBeenCalled();
});

// 同 URL 的第二次解析也拥有独立代次，慢请求不能回滚新元信息。
test("同URL并发解析仅最新代次可写", async () => {
  const pending = deferred<VideoProbe>();
  vi.mocked(api.probe).mockReturnValueOnce(pending.promise);
  videoState.url = url;
  const old = probeLink();
  await probeLink();
  pending.resolve({ ...fixture(), title: "stale" });
  await old;
  expect(videoState.probe?.title).toBe("title");
});

// 历史恢复仅填入仍有效的选择，绝不自动使用旧任务执行环境重启。
test("历史恢复参数但不启动，并拒绝迟到的恢复覆盖", async () => {
  const item = { taskId: "old", url, title: "old", state: "interrupted", updatedAt: 0,
    notePath: null, error: null, pages: [1, 2], quality: 80 };
  await restoreHistory(item);
  expect(videoState.pages).toEqual([1, 2]);
  expect(videoState.quality).toBe(80);
  expect(api.start).not.toHaveBeenCalled();
  const pending = deferred<VideoProbe>();
  vi.mocked(api.probe).mockReturnValueOnce(pending.promise);
  const restoring = restoreHistory(item);
  videoState.url = "BV0987654321";
  await probeLink();
  pending.resolve(fixture());
  await restoring;
  expect(videoState.pages).toEqual([1]);
  expect(videoState.quality).toBe(32);
});

// 退出后不会继续把前一轮扫码成功当作已登录，解析快照不参与身份判断。
test("退出清除权威登录态并保留当前选择", async () => {
  videoState.url = url;
  await probeLink();
  videoState.login = { state: "confirmed", loggedIn: true, image: "", message: "" };
  vi.mocked(api.logout).mockResolvedValue(undefined);
  await logout();
  expect(videoState.login?.loggedIn).toBe(false);
  expect(videoState.pages).toEqual([2]);
});
