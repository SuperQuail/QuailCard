import { afterEach, beforeEach, expect, test, vi } from "vitest";
import { mount } from "@vue/test-utils";
import * as api from "../../api/video";
import type { VideoLoginStatus } from "../../domain/video";
import BilibiliLoginDialog from "../../components/video/BilibiliLoginDialog.vue";
import { closeLogin, openLogin, openVideoWorkspace, videoState } from "./videoStore";

// 隔离登录测试，避免引入供应商与持久化状态。
vi.mock("./providerStore", () => ({ activeProviderId: { value: "" } }));
// 保留真实 store 与弹窗，仅替换后端命令边界。
vi.mock("../../api/video", () => ({
  loginStart: vi.fn(), loginStatus: vi.fn(), loginCancel: vi.fn(),
  getSettings: vi.fn(), components: vi.fn(), models: vi.fn(), probe: vi.fn(), history: vi.fn(),
}));

const waiting: VideoLoginStatus = { state: "waiting", message: "请扫码", image: "qr-old", loggedIn: false };
const confirmed: VideoLoginStatus = { state: "confirmed", message: "登录成功", image: "", loggedIn: true };

/** 手动控制命令完成时间，以覆盖超过轮询周期的真实网络请求。 */
function deferred<T>() {
  let resolve!: (value: T) => void;
  let reject!: (reason: unknown) => void;
  // 保存控制句柄，测试不依赖真实时钟或网络。
  const promise = new Promise<T>((yes, no) => { resolve = yes; reject = no; });
  return { promise, resolve, reject };
}

// 每个用例从独立的可重试登录态开始。
beforeEach(() => {
  vi.useFakeTimers();
  vi.resetAllMocks();
  videoState.login = null;
  videoState.loginOpen = false;
  videoState.probe = null;
  videoState.error = "";
  videoState.loading = false;
  vi.mocked(api.loginStart).mockResolvedValue(waiting);
  vi.mocked(api.loginStatus).mockResolvedValue(waiting);
  vi.mocked(api.loginCancel).mockResolvedValue(undefined);
  vi.mocked(api.components).mockResolvedValue([]);
  vi.mocked(api.models).mockResolvedValue([]);
  vi.mocked(api.history).mockResolvedValue([]);
});

// 通过公开关闭入口撤销会话，保证定时器不会泄漏到后续用例。
afterEach(async () => {
  await closeLogin();
  vi.clearAllTimers();
  vi.useRealTimers();
});

// 请求完成后才开始下一个周期，避免真实 nav 校验重叠。
test("慢速轮询只保留一个在途请求，完成后重新计时", async () => {
  const poll = deferred<VideoLoginStatus>();
  vi.mocked(api.loginStatus).mockReturnValueOnce(poll.promise);
  await openLogin();
  await vi.advanceTimersByTimeAsync(1500);
  await vi.advanceTimersByTimeAsync(15000);
  expect(api.loginStatus).toHaveBeenCalledTimes(1);
  poll.resolve({ ...waiting, state: "scanned" });
  await vi.advanceTimersByTimeAsync(0);
  await vi.advanceTimersByTimeAsync(1499);
  expect(api.loginStatus).toHaveBeenCalledTimes(1);
  await vi.advanceTimersByTimeAsync(1);
  expect(api.loginStatus).toHaveBeenCalledTimes(2);
});

// 新会话必须等待旧命令，但不允许旧确认关闭新弹窗。
test.each([false, true])("轮询在途时重新获取，旧结果不能更新新会话，失败=%s", async failure => {
  const poll = deferred<VideoLoginStatus>();
  vi.mocked(api.loginStatus).mockReturnValueOnce(poll.promise);
  await openLogin();
  await vi.advanceTimersByTimeAsync(1500);
  vi.mocked(api.loginStart).mockResolvedValue({ ...waiting, image: "qr-new" });
  const retry = openLogin();
  await vi.advanceTimersByTimeAsync(6000);
  expect(api.loginStart).toHaveBeenCalledTimes(1);
  expect(videoState.login).toBeNull();
  if (failure) poll.reject(new Error("旧轮询失败"));
  else poll.resolve(confirmed);
  await retry;
  expect(videoState.error).toBe("");
  expect(videoState.loginOpen).toBe(true);
  expect(videoState.login?.image).toBe("qr-new");
  expect(api.probe).not.toHaveBeenCalled();
});

// 关闭后的成功和失败都不能修改已经隐藏的弹窗快照。
test.each([false, true])("关闭忽略在途轮询结果，失败=%s", async failure => {
  const poll = deferred<VideoLoginStatus>();
  vi.mocked(api.loginStatus).mockReturnValueOnce(poll.promise);
  await openLogin();
  await vi.advanceTimersByTimeAsync(1500);
  const closing = closeLogin();
  expect(videoState.loginOpen).toBe(false);
  if (failure) poll.reject(new Error("旧轮询失败"));
  else poll.resolve(confirmed);
  await closing;
  expect(videoState.login).toEqual(waiting);
  expect(videoState.error).toBe("");
  await vi.advanceTimersByTimeAsync(6000);
  expect(api.loginStatus).toHaveBeenCalledTimes(1);
});

// 首次获取也可能耗时，重试必须保护新二维码免受旧错误污染。
test("在途启动失败不污染重试，队列失败后仍能继续", async () => {
  const start = deferred<VideoLoginStatus>();
  vi.mocked(api.loginStart).mockReturnValueOnce(start.promise);
  const opening = openLogin();
  await vi.advanceTimersByTimeAsync(0);
  const retry = openLogin();
  start.reject(new Error("旧启动失败"));
  await Promise.all([opening, retry]);
  expect(videoState.login).toEqual(waiting);
  expect(videoState.error).toBe("");
  expect(videoState.loginOpen).toBe(true);
});

// 旧启动、取消、新启动按序执行，取消不会误伤重新打开的后端会话。
test("启动期间关闭并重新打开，忽略旧结果且先取消再启动", async () => {
  const start = deferred<VideoLoginStatus>();
  vi.mocked(api.loginStart).mockReturnValueOnce(start.promise);
  const opening = openLogin();
  await vi.advanceTimersByTimeAsync(0);
  const closing = closeLogin();
  const reopening = openLogin();
  start.resolve(confirmed);
  await Promise.all([opening, closing, reopening]);
  expect(videoState.loginOpen).toBe(true);
  expect(videoState.login).toEqual(waiting);
  expect(api.loginCancel).toHaveBeenCalledTimes(1);
  expect(vi.mocked(api.loginCancel).mock.invocationCallOrder[0]).toBeLessThan(
    vi.mocked(api.loginStart).mock.invocationCallOrder[1],
  );
});

// 两类命令异常都必须通过原有 UI 契约显示错误、隐藏二维码并停止转圈。
test.each(["start", "poll"])("%s 失败显示消息与重试而不是加载动画", async stage => {
  const error = { code: "VIDEO_LOGIN", message: "登录校验暂不可用" };
  if (stage === "start") vi.mocked(api.loginStart).mockRejectedValueOnce(error);
  else vi.mocked(api.loginStatus).mockRejectedValueOnce(error);
  await openLogin();
  if (stage === "poll") await vi.advanceTimersByTimeAsync(1500);
  expect(videoState.login).toEqual({ state: "failed", message: error.message, image: "", loggedIn: false });
  expect(videoState.error).toBe(error.message);
  expect(videoState.loginOpen).toBe(true);
  const dialog = mount(BilibiliLoginDialog, { props: { login: videoState.login } });
  expect(dialog.text()).toContain(error.message);
  expect(dialog.text()).toContain("重新获取");
  expect(dialog.find(".animate-spin").exists()).toBe(false);
  expect(dialog.find("img").exists()).toBe(false);
  dialog.unmount();
  const calls = vi.mocked(api.loginStatus).mock.calls.length;
  await vi.advanceTimersByTimeAsync(6000);
  expect(api.loginStatus).toHaveBeenCalledTimes(calls);
});

// 工作区补查失败必须可见，且不能让加载状态卡住。
test("打开工作区不再吞掉登录校验异常", async () => {
  vi.mocked(api.loginStatus).mockRejectedValueOnce(new Error("Cookie 校验失败"));
  await openVideoWorkspace();
  expect(videoState.error).toBe("Cookie 校验失败");
  expect(videoState.login?.state).toBe("failed");
  expect(videoState.loading).toBe(false);
});

// 工作区校验与弹窗启动共用队列，旧工作区异常不能污染新弹窗。
test("工作区慢速校验之后打开登录，忽略旧异常", async () => {
  const status = deferred<VideoLoginStatus>();
  vi.mocked(api.loginStatus).mockReturnValueOnce(status.promise);
  const workspace = openVideoWorkspace();
  await vi.advanceTimersByTimeAsync(0);
  const opening = openLogin();
  await vi.advanceTimersByTimeAsync(3000);
  expect(api.loginStart).not.toHaveBeenCalled();
  status.reject(new Error("旧工作区校验失败"));
  await Promise.all([workspace, opening]);
  expect(videoState.error).toBe("");
  expect(videoState.login).toEqual(waiting);
  expect(videoState.loginOpen).toBe(true);
});

// 弹窗已经负责轮询时，工作区补查不能额外启动同一后端状态命令。
test("弹窗打开时工作区不重复校验登录", async () => {
  await openLogin();
  await openVideoWorkspace();
  expect(api.loginStatus).not.toHaveBeenCalled();
  await vi.advanceTimersByTimeAsync(1500);
  expect(api.loginStatus).toHaveBeenCalledTimes(1);
});

// 恢复的有效凭据必须显示已登录，且不再创建二维码。
test("工作区恢复登录结果且已登录不申请二维码", async () => {
  vi.mocked(api.loginStatus).mockResolvedValueOnce(confirmed);
  await openVideoWorkspace();
  expect(videoState.login).toEqual(confirmed);
  await openLogin();
  expect(api.loginStart).not.toHaveBeenCalled();
  expect(videoState.loginOpen).toBe(false);
});

// 真实确认保持原有自动关闭契约，并且不再安排轮询。
test("确认登录后自动关闭并停止轮询", async () => {
  vi.mocked(api.loginStatus).mockResolvedValueOnce(confirmed);
  await openLogin();
  await vi.advanceTimersByTimeAsync(1500);
  expect(videoState.login).toEqual(confirmed);
  expect(videoState.loginOpen).toBe(false);
  await vi.advanceTimersByTimeAsync(6000);
  expect(api.loginStatus).toHaveBeenCalledTimes(1);
});
