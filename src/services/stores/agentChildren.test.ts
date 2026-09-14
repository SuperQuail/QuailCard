import { afterEach, beforeEach, expect, test, vi } from "vitest";
import type { AgentChildInfo, AgentRun, AgentSession } from "../../domain/agent";
import { createAgentChildren, type AgentChildrenState } from "./agentChildren";
import * as api from "../../api/agent";

vi.mock("../../api/agent", () => ({ children: vi.fn(), interruptChild: vi.fn(), messageChild: vi.fn() }));

/** 最小 DTO 保留父链，避免把无关会话当成可管理子代理。 */
function session(id: string, parentSessionId?: string): AgentSession {
  return { id, parentSessionId, formatVersion: 1, title: id, updatedAt: 0, messages: [], summary: "", selectedPaths: [] };
}
/** 用可控 Promise 复现切会话和关闭详情期间的网络迟到。 */
function deferred<T>() {
  let resolve!: (value: T) => void, reject!: (reason: unknown) => void;
  const promise = new Promise<T>((yes, no) => { resolve = yes; reject = no; });
  return { promise, resolve, reject };
}
/** 当前运行只能绑定 root，不能借用别的会话的活动任务。 */
function run(sessionId = "root"): AgentRun {
  return { id: "run", sessionId, state: "running", sequence: 1, text: "", phase: "", pendingWrite: null, pendingWriteId: null, error: null, reasoning: "", reasoningMessageId: "" };
}
const child: AgentChildInfo = { agentId: "child", parentSessionId: "root", delegationDepth: 1, description: "检索资料", status: "ready" };
let state: AgentChildrenState;
let human: ReturnType<typeof vi.fn<(content: string, id: string) => Promise<void>>>;
let flow: ReturnType<typeof createAgentChildren>;

/** 独立实例不共享 epoch，所有测试只验证前端契约而非重写宿主调度。 */
beforeEach(() => {
  vi.resetAllMocks();
  state = { session: session("root"), run: null, loading: false, sending: false, children: [{ ...child }], childrenError: "" };
  human = vi.fn().mockResolvedValue(undefined);
  flow = createAgentChildren(state, human);
  vi.mocked(api.children).mockResolvedValue([{ ...child }]);
});
/** 时间桩只约束当前测试的节流窗口。 */
afterEach(() => { vi.restoreAllMocks(); });

/** 后台轮询节流，但显式刷新和运行结束必须立即更新。 */
test("root poll 节流并且只携带匹配的活动 run", async () => {
  const now = vi.spyOn(Date, "now").mockReturnValue(1000);
  state.run = run();
  await flow.refreshChildren(false); await flow.refreshChildren(false);
  expect(api.children).toHaveBeenCalledTimes(1);
  expect(api.children).toHaveBeenCalledWith("root", "run");
  now.mockReturnValue(2500); await flow.refreshChildren(false);
  expect(api.children).toHaveBeenCalledTimes(2);
  state.run.state = "waiting"; await flow.refreshChildren();
  expect(api.children).toHaveBeenLastCalledWith("root", null);
});

/** 切离再返回相同 ID 也必须丢弃旧 epoch 的列表。 */
test("会话切换 epoch 隔离迟到列表及错误", async () => {
  const old = deferred<AgentChildInfo[]>();
  vi.mocked(api.children).mockReturnValueOnce(old.promise);
  const refresh = flow.refreshChildren(); await Promise.resolve();
  flow.reset(); state.session = session("other");
  flow.reset(); state.session = session("root");
  const latest = flow.refreshChildren();
  expect(api.children).toHaveBeenCalledTimes(1);
  old.resolve([{ ...child, agentId: "stale" }]); await Promise.all([refresh, latest]);
  expect(state.children).toEqual([child]);
  const failure = deferred<AgentChildInfo[]>();
  vi.mocked(api.children).mockReturnValueOnce(failure.promise);
  const failed = flow.refreshChildren(); await Promise.resolve(); flow.reset(); state.session = session("new");
  failure.reject(new Error("secret")); await failed;
  expect(state.childrenError).toBe(""); expect(state.children).toEqual([]);
});

/** 手动刷新合并为串行后续请求，不让并发扫描积压或旧结果覆盖终态。 */
test("强制刷新合并且重复点击不会并发读取", async () => {
  const old = deferred<AgentChildInfo[]>();
  vi.mocked(api.children).mockReturnValueOnce(old.promise);
  const pending = flow.refreshChildren(false); await Promise.resolve();
  const forced = flow.refreshChildren(); void flow.refreshChildren();
  expect(api.children).toHaveBeenCalledTimes(1);
  old.resolve([]); await Promise.all([pending, forced]);
  expect(api.children).toHaveBeenCalledTimes(2); expect(state.children).toEqual([child]);
});

/** 相同摘要复用引用，展开的子树不会因定时查询反复重建。 */
test("无变化的子列表保留数组和行对象", async () => {
  const original = state.children; await flow.refreshChildren();
  expect(state.children).toBe(original);
});

/** 列表失败只保留安全文案，原始异常与凭据不进入界面。 */
test("列表错误安全展示且不泄露原始异常", async () => {
  vi.mocked(api.children).mockRejectedValueOnce(new Error("secret-key=/private/file"));
  await flow.refreshChildren(); expect(state.childrenError).toBe("无法刷新子代理，请重试。");
});

/** 正在执行的 root 可直接追问直接子，不生成任何虚构后台消息。 */
test("活动树追问与中断使用当前运行身份", async () => {
  state.run = run(); state.sending = true; state.children[0].status = "running";
  vi.mocked(api.children).mockResolvedValue([{ ...child, status: "running" }]);
  await flow.messageChild("child", "继续检索"); await flow.interruptChild("child");
  expect(api.messageChild).toHaveBeenCalledWith("run", "child", "继续检索");
  expect(api.interruptChild).toHaveBeenCalledWith("run", "child"); expect(human).not.toHaveBeenCalled();
});

/** 闲置 root 必须由真实人类文本启动，再让父模型负责冷恢复。 */
test("闲置root追问走明确人类继续请求", async () => {
  await flow.messageChild("child", "补充证据");
  expect(api.messageChild).not.toHaveBeenCalled();
  expect(human).toHaveBeenCalledWith(expect.stringContaining("send_message"), "root");
  expect(human.mock.calls[0][0]).toContain("补充证据");
});

/** 孙级只读、陈旧 run、空消息与 UTF-8 超限内容都不应产生副作用。 */
test("校验直接父链、当前run与16KiB字节上限", async () => {
  state.children.push({ ...child, agentId: "grandchild", parentSessionId: "child", delegationDepth: 2 });
  await flow.messageChild("grandchild", "继续"); await flow.messageChild("child", "  ");
  await flow.messageChild("child", "中".repeat(5500));
  expect(state.childrenError).toContain("16 KiB"); expect(human).not.toHaveBeenCalled();
  state.run = run("another"); await flow.messageChild("child", "继续"); await flow.interruptChild("child");
  expect(api.messageChild).not.toHaveBeenCalled(); expect(api.interruptChild).not.toHaveBeenCalled();
  state.run = null; await flow.messageChild("child", "a".repeat(16384));
  expect(human).toHaveBeenCalledTimes(1);
});

/** 目标只有点击恢复才发送人类继续请求，不把存储的 active 误认成许可。 */
test("resumeGoal显式继续且不修改目标状态", async () => {
  state.session!.goal = { id: "goal", revision: 3, objective: "完成学习", acceptanceCriteria: [], phase: "paused", roundsStarted: 1, maxGoalRounds: 5, evidence: [], blocker: null };
  await flow.refreshChildren(); expect(human).not.toHaveBeenCalled();
  await flow.resumeGoal(); expect(human).toHaveBeenCalledWith(expect.stringContaining("请继续当前会话的目标"), "root");
  expect(state.session!.goal.phase).toBe("paused");
  state.session!.goal.phase = "complete"; await flow.resumeGoal(); expect(human).toHaveBeenCalledTimes(1);
});

/** 操作失败只能显示固定安全消息，迟到失败不得污染新 root。 */
test("追问错误安全展示且迟到中断错误被隔离", async () => {
  state.run = run(); state.children[0].status = "running";
  vi.mocked(api.messageChild).mockRejectedValueOnce(new Error("token=secret"));
  await flow.messageChild("child", "继续"); expect(state.childrenError).toBe("发送子代理追问失败，请重试。");
  const interruption = deferred<void>(); vi.mocked(api.interruptChild).mockReturnValueOnce(interruption.promise);
  const pending = flow.interruptChild("child"); flow.reset(); state.session = session("new");
  interruption.reject(new Error("private")); await pending; expect(state.childrenError).toBe("");
});

/** 根切换只能替换后继查询目标，不清除尚在途的物理请求。 */
test("慢目录查询期间连续重置只串行查询最后一个根", async () => {
  const old = deferred<AgentChildInfo[]>(); vi.mocked(api.children).mockReturnValueOnce(old.promise);
  const first = flow.refreshChildren(); await Promise.resolve(); let latest = first;
  for (let index = 0; index < 50; index += 1) {
    flow.reset(); state.session = session("root-" + index); state.run = null;
    latest = flow.refreshChildren();
  }
  await Promise.resolve(); expect(api.children).toHaveBeenCalledTimes(1);
  const lastChild = { ...child, parentSessionId: "root-49" }; vi.mocked(api.children).mockResolvedValue([lastChild]);
  old.resolve([child]); await Promise.all([first, latest]);
  expect(api.children).toHaveBeenCalledTimes(2); expect(api.children).toHaveBeenLastCalledWith("root-49", null);
  expect(state.children).toEqual([lastChild]);
});
