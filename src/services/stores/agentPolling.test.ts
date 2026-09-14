import { afterEach, beforeEach, expect, test, vi } from "vitest";
import type { AgentObservation, AgentPendingWrite, AgentRun, AgentSession } from "../../domain/agent";
vi.mock("../../api/agent", () => ({ observe: vi.fn(), pendingWrites: vi.fn(), children: vi.fn(), send: vi.fn(), status: vi.fn(), session: vi.fn(), sessions: vi.fn(), cancel: vi.fn(), acknowledgeWrite: vi.fn() }));
vi.mock("./providerStore", () => ({ activeProviderId: { value: "model" } }));
vi.mock("./noteStore", () => ({ noteOperationBusy: { value: false }, notePersistence: { flushAll: vi.fn() }, activeNotePath: { value: "" } }));
vi.mock("../agentFiles", () => ({ coordinateAgentWrite: vi.fn(async (action: () => Promise<void>) => action()), undoAgentChange: vi.fn() }));
vi.mock("../agentReview", () => ({ clearAgentReviews: vi.fn(), agentReviewsBusy: () => false, settleAgentReviews: vi.fn() }));
vi.mock("./cardStore", () => ({ loadActiveCards: vi.fn(), reloadActiveCards: vi.fn() }));
vi.mock("./reviewStore", () => ({ refreshStats: vi.fn() }));
import * as api from "../../api/agent";
import { noteOperationBusy } from "./noteStore";
import { agentChildren, agentChildDetail, agentState, sendAgentMessage, stopAgent } from "./agentStore";

/** 版本化会话只在持久内容变化时有新消息。 */
function session(id = "root", content = "旧消息"): AgentSession {
  return { id, title: id, selectedPaths: [], messages: [{ id: "m", role: "assistant", kind: "text", content, data: null }], summary: "", updatedAt: 1, formatVersion: 1 };
}
let requestId = "";
/** 快照身份由实际发送请求确定，序号与历史版本分别变化。 */
function run(sequence: number, state: AgentRun["state"] = "running"): AgentRun {
  return { id: requestId, sessionId: "root", state, sequence, text: "实时输出", phase: "处理中", pendingWrite: null, pendingWriteId: null, error: null, reasoning: "", reasoningMessageId: "" };
}
/** 测试桩只提供宿主事实，不在前端重做执行规则。 */
function observation(revision: string, history: AgentSession | null, current: AgentRun, writes: AgentPendingWrite[] = []): AgentObservation {
  return { sessionId: "root", revision, session: history, run: current, writes };
}
/** 整树写入列表由后端提供，前端只按执行身份与操作身份确认。 */
function write(executionId: string, operationId: string, path = "note.md"): AgentPendingWrite {
  return { executionId, sessionId: executionId === requestId ? "root" : "child", path, operationId };
}
/** 固定刷新时钟以准确统计全量读取次数。 */
beforeEach(() => {
  vi.useFakeTimers(); vi.clearAllMocks(); agentChildren.reset(); agentChildDetail.closeChild();
  noteOperationBusy.value = false;
  Object.assign(agentState, { open: false, loading: false, sending: false, readingImages: false, run: null, session: session(), sessions: [], draft: "保留", images: [], selectedPaths: [], error: "" });
  vi.mocked(api.children).mockResolvedValue([]); vi.mocked(api.session).mockResolvedValue(session());
  vi.mocked(api.pendingWrites).mockResolvedValue([]);
  vi.mocked(api.sessions).mockResolvedValue([session()]);
  vi.mocked(api.send).mockImplementation(async input => { requestId = input.requestId; return run(1); });
});
/** 每个发送必须收尾，恢复时钟不会留下下一例的刷新任务。 */
afterEach(() => { agentChildDetail.closeChild(); vi.useRealTimers(); });

test("无变化历史不重读或替换，状态与终态仍正常推进", async () => {
  let count = 0;
  vi.mocked(api.observe).mockImplementation(async () => {
    count += 1;
    return count === 1 ? observation("v1", session(), run(2))
      : count === 2 ? observation("v1", null, run(3))
      : observation("v2", session("root", "最终结果"), run(4, "completed"));
  });
  const sending = sendAgentMessage("开始"); await vi.advanceTimersByTimeAsync(0);
  const initial = agentState.session;
  await vi.advanceTimersByTimeAsync(250);
  expect(agentState.session).toBe(initial); expect(agentState.run?.sequence).toBe(3);
  expect(api.observe).toHaveBeenLastCalledWith("root", requestId, null, "v1");
  await vi.advanceTimersByTimeAsync(250); await sending;
  expect(agentState.session?.messages[0].content).toBe("最终结果");
  expect(agentState.sending).toBe(false); expect(api.session).not.toHaveBeenCalled();
  expect(api.observe).toHaveBeenCalledTimes(3); expect(api.status).not.toHaveBeenCalled();
});

test("历史读取失败仍可通过轻量状态结束取消握手", async () => {
  vi.mocked(api.observe).mockRejectedValue(new Error("记录暂时不可用"));
  vi.mocked(api.status).mockImplementation(async () => run(2, "cancelled"));
  await sendAgentMessage("开始");
  expect(agentState.run?.state).toBe("cancelled"); expect(agentState.sending).toBe(false);
  expect(agentState.error).toBe("记录暂时不可用"); expect(api.status).toHaveBeenCalledTimes(1);
});

test("未持有历史的版本变更不允许伪装unchanged", async () => {
  vi.mocked(api.observe).mockImplementation(async () => observation("unknown", null, run(2)));
  vi.mocked(api.status).mockImplementation(async () => run(3, "failed"));
  await sendAgentMessage("开始");
  expect(agentState.session?.messages[0].content).toBe("旧消息");
  expect(agentState.error).toContain("不匹配"); expect(agentState.run?.state).toBe("failed");
});

test("根保存等待经整树写入列表确认", async () => {
  let count = 0;
  vi.mocked(api.observe).mockImplementation(async () => {
    count += 1;
    return count === 1 ? observation("v1", session(), run(2), [write(requestId, "operation")])
      : observation("v2", session(), run(3, "completed"));
  });
  const sending = sendAgentMessage("保存"); await vi.advanceTimersByTimeAsync(0);
  expect(api.acknowledgeWrite).toHaveBeenCalledWith(requestId, requestId, "operation");
  await vi.advanceTimersByTimeAsync(250); await sending;
  expect(agentState.run?.state).toBe("completed");
});

test("子代理保存等待不打开详情也能确认", async () => {
  let count = 0;
  vi.mocked(api.observe).mockImplementation(async () => {
    count += 1;
    return count === 1
      ? observation("v1", session(), run(2), [write(requestId, "operation"), write("child-run", "child-operation", "笔记.md")])
      : observation("v2", session(), run(3, "completed"));
  });
  const sending = sendAgentMessage("委派"); await vi.advanceTimersByTimeAsync(0);
  expect(agentState.childDetail).toBeNull();
  expect(api.acknowledgeWrite).toHaveBeenNthCalledWith(1, requestId, requestId, "operation");
  expect(api.acknowledgeWrite).toHaveBeenNthCalledWith(2, requestId, "child-run", "child-operation");
  await vi.advanceTimersByTimeAsync(250); await sending;
  expect(agentState.run?.state).toBe("completed"); expect(agentState.childDetail).toBeNull();
});

test("历史读取失败时改用只读整树查询完成保存协调", async () => {
  let count = 0;
  vi.mocked(api.observe).mockRejectedValue(new Error("记录暂时不可用"));
  vi.mocked(api.status).mockImplementation(async () => (count += 1) === 1 ? run(2) : run(3, "completed"));
  // 根快照没有待写入时，子代理的保存等待只能来自整树查询。
  vi.mocked(api.pendingWrites).mockImplementation(async () => (count === 1 ? [write("child-run", "child-operation")] : []));
  const sending = sendAgentMessage("保存"); await vi.advanceTimersByTimeAsync(0);
  expect(api.pendingWrites).toHaveBeenCalledWith(requestId);
  expect(api.acknowledgeWrite).toHaveBeenCalledWith(requestId, "child-run", "child-operation");
  // 历史读取失败会进入退避，下一轮按延长间隔重试。
  await vi.advanceTimersByTimeAsync(1500); await sending;
  expect(agentState.run?.state).toBe("completed");
});

test("整树查询也失败时仍用根快照兜底", async () => {
  let count = 0;
  vi.mocked(api.observe).mockRejectedValue(new Error("记录暂时不可用"));
  vi.mocked(api.pendingWrites).mockRejectedValue(new Error("整树查询失败"));
  vi.mocked(api.status).mockImplementation(async () => (count += 1) === 1
    ? { ...run(2), pendingWrite: "note.md", pendingWriteId: "operation" } : run(3, "completed"));
  const sending = sendAgentMessage("保存"); await vi.advanceTimersByTimeAsync(0);
  expect(api.acknowledgeWrite).toHaveBeenCalledWith(requestId, requestId, "operation");
  await vi.advanceTimersByTimeAsync(1500); await sending;
  expect(agentState.run?.state).toBe("completed");
});

test("编辑器正忙时保留写入等待并在下一轮重试", async () => {
  noteOperationBusy.value = true;
  let count = 0;
  vi.mocked(api.observe).mockImplementation(async () => {
    count += 1;
    return count <= 2 ? observation("v" + count, session(), run(count + 1), [write(requestId, "operation")])
      : observation("v" + count, session(), run(9, "completed"));
  });
  const sending = sendAgentMessage("保存"); await vi.advanceTimersByTimeAsync(0);
  expect(api.acknowledgeWrite).not.toHaveBeenCalled(); expect(api.cancel).not.toHaveBeenCalled();
  noteOperationBusy.value = false;
  await vi.advanceTimersByTimeAsync(250);
  expect(api.acknowledgeWrite).toHaveBeenCalledWith(requestId, requestId, "operation");
  await vi.advanceTimersByTimeAsync(250); await sending;
  expect(agentState.run?.state).toBe("completed");
});

test("迟到的根观察不能覆盖新根会话和草稿", async () => {
  let finish!: (value: AgentObservation) => void;
  vi.mocked(api.observe).mockReturnValueOnce(new Promise(resolve => { finish = resolve; }));
  const sending = sendAgentMessage("开始"); await vi.advanceTimersByTimeAsync(0);
  agentState.session = session("other", "新会话"); agentState.draft = "新草稿";
  finish(observation("v1", session(), run(2, "completed"))); await sending;
  expect(agentState.session.id).toBe("other"); expect(agentState.draft).toBe("新草稿");
});

/** 多次停止同一任务共用物理握手，低序号状态不能复活终态。 */
test("重复停止共享请求且不接受倒退序号", async () => {
  requestId = "existing"; agentState.run = run(10);
  let finish!: (value: AgentRun) => void;
  vi.mocked(api.cancel).mockResolvedValue();
  vi.mocked(api.status).mockReturnValueOnce(new Promise(resolve => { finish = resolve; }));
  const first = stopAgent(), second = stopAgent(); expect(first).toBe(second);
  await vi.advanceTimersByTimeAsync(0);
  expect(api.cancel).toHaveBeenCalledTimes(1); expect(api.status).toHaveBeenCalledTimes(1);
  agentState.run = run(12, "completed"); finish(run(11)); await first;
  expect(agentState.run.state).toBe("completed"); expect(agentState.run.sequence).toBe(12);
});

/** 等待停止状态时离开当前身份，旧回包不能写回新会话或空工作区。 */
test("迟到停止状态不会污染其他会话", async () => {
  requestId = "existing"; agentState.run = run(1);
  let finish!: (value: AgentRun) => void;
  vi.mocked(api.cancel).mockResolvedValue();
  vi.mocked(api.status).mockReturnValueOnce(new Promise(resolve => { finish = resolve; }));
  const stopping = stopAgent(); await vi.advanceTimersByTimeAsync(0);
  agentState.session = session("other"); agentState.run = null;
  finish(run(2, "cancelled")); await stopping;
  expect(agentState.run).toBeNull(); expect(agentState.session.id).toBe("other");
});

/** 同根新请求不增加会话 generation，停止去重必须另外绑定发送 operation。 */
test("旧取消响应迟到时同根新任务仍能独立停止", async () => {
  let finishFirst!: (value: AgentObservation) => void, finishSecond!: (value: AgentObservation) => void;
  let releaseCancel!: () => void;
  vi.mocked(api.observe).mockReturnValueOnce(new Promise(resolve => { finishFirst = resolve; }))
    .mockReturnValueOnce(new Promise(resolve => { finishSecond = resolve; }));
  vi.mocked(api.cancel).mockReturnValueOnce(new Promise<void>(resolve => { releaseCancel = resolve; })).mockResolvedValue(undefined);
  const firstSend = sendAgentMessage("第一轮"); await vi.advanceTimersByTimeAsync(0);
  const firstId = requestId, firstFinal = observation("a", session(), run(2, "completed"));
  const firstStop = stopAgent(); await vi.advanceTimersByTimeAsync(0);
  finishFirst(firstFinal); await firstSend;
  const secondSend = sendAgentMessage("第二轮"); await vi.advanceTimersByTimeAsync(0);
  const secondId = requestId, secondStop = stopAgent(); await vi.advanceTimersByTimeAsync(0);
  expect(firstStop).not.toBe(secondStop);
  expect(api.cancel).toHaveBeenNthCalledWith(1, firstId); expect(api.cancel).toHaveBeenNthCalledWith(2, secondId);
  releaseCancel(); await firstStop;
  expect(agentState.run?.id).toBe(secondId); expect(agentState.run?.state).toBe("running");
  finishSecond(observation("b", session(), run(2, "cancelled"))); await Promise.all([secondSend, secondStop]);
  expect(agentState.run?.state).toBe("cancelled"); expect(api.status).not.toHaveBeenCalled();
});
