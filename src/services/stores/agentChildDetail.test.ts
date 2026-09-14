import { afterEach, beforeEach, expect, test, vi } from "vitest";
import { reactive } from "vue";
import type { AgentObservation, AgentRun, AgentSession } from "../../domain/agent";
import * as api from "../../api/agent";
import { createAgentChildDetail, type AgentChildDetailHost } from "./agentChildDetail";

vi.mock("../../api/agent", () => ({ observe: vi.fn() }));
/** 最小会话夹具只含展示事实，不模拟后端派生或授权。 */
function session(id: string): AgentSession { return { id, formatVersion: 1, title: id, messages: [], selectedPaths: [], summary: "", updatedAt: 1 }; }
/** 同一根与子任务拥有不同执行身份。 */
function run(sessionId: string, state: AgentRun["state"] = "running", sequence = 1): AgentRun {
  return { id: sessionId + "-run", sessionId, state, sequence, text: "输出", phase: "读取资料", pendingWrite: null, pendingWriteId: null, error: null, reasoning: "", reasoningMessageId: "" };
}
/** 版本和正文独立于实时状态，以便验证无变化历史不会替换。 */
function observation(id = "child", revision = "v1"): AgentObservation { return { sessionId: id, revision, session: session(id), run: run(id), writes: [] }; }
/** 延迟返回用于模拟快速切换后的旧请求。 */
function deferred<T>() { let resolve!: (value: T) => void; const promise = new Promise<T>(yes => { resolve = yes; }); return { promise, resolve }; }
let state: AgentChildDetailHost;
let flow: ReturnType<typeof createAgentChildDetail>;
/** 每例只创建一个受控观察器，并以假时间验证刷新数量。 */
beforeEach(() => {
  vi.useFakeTimers(); vi.resetAllMocks();
  state = reactive({ open: true, session: session("root"), run: run("root"), childDetail: null });
  flow = createAgentChildDetail(state);
  vi.mocked(api.observe).mockResolvedValue(observation());
});
/** 详情关闭必须释放计时器，避免测试和真实工作区生命周期泄漏。 */
afterEach(() => { flow.closeChild(); vi.useRealTimers(); });

test("父任务运行中可打开独立子详情，不改父会话与执行", async () => {
  const parent = state.session, parentRun = state.run;
  await flow.openChild("child");
  expect(api.observe).toHaveBeenCalledWith("root", "root-run", "child", null);
  expect(state.childDetail?.session?.id).toBe("child");
  expect(state.session).toBe(parent); expect(state.run).toBe(parentRun);
  expect(state.childDetail?.loading).toBe(false);
});

test("相同历史版本保留对象，实时输出仍更新", async () => {
  await flow.openChild("child"); const history = state.childDetail!.session;
  vi.mocked(api.observe).mockResolvedValue({ sessionId: "child", revision: "v1", session: null, run: run("child", "running", 2), writes: [] });
  await flow.refreshChild();
  expect(api.observe).toHaveBeenLastCalledWith("root", "root-run", "child", "v1");
  expect(state.childDetail!.session).toBe(history); expect(state.childDetail!.run?.sequence).toBe(2);
});

test("自动与手动刷新共享一个进行中请求，关闭取消后续刷新", async () => {
  await flow.openChild("child"); const delayed = deferred<AgentObservation>();
  vi.mocked(api.observe).mockReturnValueOnce(delayed.promise);
  const first = flow.refreshChild(), second = flow.refreshChild();
  expect(first).toBe(second); await Promise.resolve();
  expect(api.observe).toHaveBeenCalledTimes(2);
  flow.closeChild(); delayed.resolve(observation()); await first;
  await vi.advanceTimersByTimeAsync(10000);
  expect(state.childDetail).toBeNull(); expect(api.observe).toHaveBeenCalledTimes(2);
});

test("快速切换子代理后迟到结果不串入新详情", async () => {
  const delayed = deferred<AgentObservation>(); vi.mocked(api.observe).mockReturnValueOnce(delayed.promise);
  const old = flow.openChild("old"); await Promise.resolve();
  vi.mocked(api.observe).mockResolvedValue(observation("new")); const latest = flow.openChild("new");
  expect(api.observe).toHaveBeenCalledTimes(1);
  delayed.resolve(observation("old")); await Promise.all([old, latest]);
  expect(state.childDetail?.id).toBe("new"); expect(state.childDetail?.session?.id).toBe("new");
});

test("同一身份关闭重开也丢弃旧响应", async () => {
  const delayed = deferred<AgentObservation>(); vi.mocked(api.observe).mockReturnValueOnce(delayed.promise);
  const old = flow.openChild("child"); await Promise.resolve(); flow.closeChild();
  vi.mocked(api.observe).mockResolvedValue(observation("child", "new")); const latest = flow.openChild("child");
  expect(api.observe).toHaveBeenCalledTimes(1);
  delayed.resolve(observation("child", "old")); await Promise.all([old, latest]);
  await flow.refreshChild(); expect(api.observe).toHaveBeenLastCalledWith("root", "root-run", "child", "new");
});

test("父任务结束后最后一次刷新回退历史，停止定时查询", async () => {
  await flow.openChild("child");
  vi.mocked(api.observe).mockResolvedValue({ ...observation(), run: null });
  state.run = run("root", "completed"); flow.sync(); await flow.refreshChild();
  expect(api.observe).toHaveBeenLastCalledWith("root", null, "child", "v1");
  expect(state.childDetail?.session?.id).toBe("child"); expect(state.childDetail?.run).toBeNull();
  const count = vi.mocked(api.observe).mock.calls.length;
  await vi.advanceTimersByTimeAsync(10000); expect(api.observe).toHaveBeenCalledTimes(count);
});

test("根切换或工作区隐藏即关闭详情且不保留观察任务", async () => {
  await flow.openChild("child"); state.open = false; flow.sync();
  expect(state.childDetail).toBeNull(); await vi.advanceTimersByTimeAsync(10000);
  expect(api.observe).toHaveBeenCalledTimes(1);
  state.open = true; await flow.openChild("child"); state.session = session("other"); flow.sync();
  expect(state.childDetail).toBeNull();
});

test("根执行代次改变时，旧执行输出不得污染当前详情", async () => {
  await flow.openChild("child"); const delayed = deferred<AgentObservation>();
  vi.mocked(api.observe).mockReturnValueOnce(delayed.promise); const old = flow.refreshChild(); await Promise.resolve();
  state.run = { ...run("root"), id: "next-run" };
  vi.mocked(api.observe).mockResolvedValue({ ...observation(), run: { ...run("child"), id: "child-next" } });
  flow.sync(); const latest = flow.refreshChild();
  expect(api.observe).toHaveBeenCalledTimes(2);
  delayed.resolve(observation()); await Promise.all([old, latest]);
  expect(state.childDetail?.run?.id).toBe("child-next");
});

test("错身份或无历史的unchanged响应显示安全错误，不接纳返回内容", async () => {
  vi.mocked(api.observe).mockResolvedValue(observation("unrelated")); await flow.openChild("child");
  expect(state.childDetail?.session).toBeNull(); expect(state.childDetail?.error).toContain("无法读取");
  vi.mocked(api.observe).mockResolvedValue({ ...observation(), session: null }); await flow.refreshChild();
  expect(state.childDetail?.session).toBeNull();
  vi.mocked(api.observe).mockRejectedValue(new Error("token=private")); await flow.refreshChild();
  expect(state.childDetail?.error).not.toContain("private");
});

/** 新旧展示代次共用物理请求槽，不能把不可取消的 IPC 当作已取消。 */
test("慢请求期间反复关闭重开只保留最后一个目标", async () => {
  const delayed = deferred<AgentObservation>(); vi.mocked(api.observe).mockReturnValueOnce(delayed.promise);
  const first = flow.openChild("first"); await Promise.resolve();
  let latest: Promise<void> = first;
  for (let index = 0; index < 50; index += 1) { flow.closeChild(); latest = flow.openChild("child-" + index); }
  await Promise.resolve(); expect(api.observe).toHaveBeenCalledTimes(1);
  vi.mocked(api.observe).mockResolvedValue(observation("child-49"));
  delayed.resolve(observation("first")); await Promise.all([first, latest]);
  expect(api.observe).toHaveBeenCalledTimes(2); expect(state.childDetail?.session?.id).toBe("child-49");
});

/** 微任务尚未执行就关闭时，不发送空根身份或已经废弃的目标请求。 */
test("同一tick打开再关闭不调用观察接口", async () => {
  const opening = flow.openChild("child"); flow.closeChild(); await opening;
  expect(api.observe).not.toHaveBeenCalled(); expect(state.childDetail).toBeNull();
});
