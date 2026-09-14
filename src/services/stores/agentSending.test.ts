import { beforeEach, expect, test, vi } from "vitest";
import type { AgentRun, AgentSession } from "../../domain/agent";

vi.mock("../../api/agent", () => ({ children: vi.fn().mockResolvedValue([]), send: vi.fn(), status: vi.fn(), session: vi.fn(), sessions: vi.fn(), createSession: vi.fn() }));
vi.mock("./providerStore", () => ({ activeProviderId: { value: "model" } }));
vi.mock("./noteStore", () => ({ noteOperationBusy: { value: false }, notePersistence: { flushAll: vi.fn() }, activeNotePath: { value: "" } }));
vi.mock("../agentFiles", () => ({ coordinateAgentWrite: vi.fn(), undoAgentChange: vi.fn() }));
vi.mock("../agentReview", () => ({ clearAgentReviews: vi.fn(), agentReviewsBusy: () => false, settleAgentReviews: vi.fn() }));
vi.mock("./cardStore", () => ({ loadActiveCards: vi.fn(), reloadActiveCards: vi.fn() }));
vi.mock("./reviewStore", () => ({ refreshStats: vi.fn() }));
import * as api from "../../api/agent";
import { notePersistence } from "./noteStore";
import { settleAgentReviews } from "../agentReview";
import { agentChildren, agentState, sendAgentMessage, selectAgentSession, stopAgent, leaveAgentVault } from "./agentStore";

/** 隔离会话身份，验证附件不会跟随错误的对话发送。 */
function session(id: string): AgentSession { return { id, title: id, selectedPaths: [], messages: [], summary: "", updatedAt: 1, formatVersion: 1 }; }
const image = { name: "paste.png", mimeType: "image/png", dataBase64: "aW1hZ2U=" };

/** 每例重置网络行为，发送完成由后端状态而非前端清空模拟。 */
beforeEach(() => {
  vi.resetAllMocks(); agentChildren.reset();
  vi.mocked(api.children).mockResolvedValue([]);
  Object.assign(agentState, { loading: false, sending: false, readingImages: false, run: null, session: session("a"), sessions: [session("a")], draft: "", selectedPaths: [], images: [{ ...image }], error: "" });
  vi.mocked(api.session).mockImplementation(async id => session(id));
  vi.mocked(api.sessions).mockResolvedValue([session("a"), session("b")]);
});

/** 附件仅由用户聊天发送，后台状态决定登记是否成功。 */
test("纯图片随请求发送，成功登记后才清空附件", async () => {
  vi.mocked(api.send).mockImplementation(async input => ({ id: input.requestId, sessionId: "a", state: "completed", sequence: 1, text: "", phase: "", pendingWrite: null, pendingWriteId: null, error: null, reasoning: "", reasoningMessageId: "" } as AgentRun));
  await sendAgentMessage();
  expect(api.send).toHaveBeenCalledWith(expect.objectContaining({ content: "", images: [image] }));
  expect(agentState.images).toEqual([]);
});

test("发送失败保留附件和错误，编码期间不发送", async () => {
  vi.mocked(api.send).mockRejectedValue(new Error("发送失败"));
  vi.mocked(api.status).mockRejectedValue(new Error("未登记"));
  await sendAgentMessage();
  expect(agentState.images).toEqual([image]);
  expect(agentState.error).toBe("发送失败");
  agentState.readingImages = true;
  await sendAgentMessage();
  expect(api.send).toHaveBeenCalledTimes(1);
});

test("会话切换隔离图片草稿并在返回时恢复", async () => {
  await selectAgentSession("b");
  expect(agentState.images).toEqual([]);
  await selectAgentSession("a");
  expect(agentState.images).toEqual([image]);
});

/** 管理发送共用后端任务流，但绝不消费尚未发送的文本与图片。 */
test("resumeGoal保留草稿图片且只发明确继续文本", async () => {
  agentState.draft = "尚未发送的草稿";
  agentState.session!.goal = { id: "goal", revision: 1, objective: "学习", acceptanceCriteria: [], phase: "paused", roundsStarted: 1, maxGoalRounds: 5, evidence: [], blocker: null };
  vi.mocked(api.send).mockImplementation(async input => ({ id: input.requestId, sessionId: input.sessionId, state: "completed", sequence: 1 } as AgentRun));
  await agentChildren.resumeGoal();
  expect(api.send).toHaveBeenCalledWith(expect.objectContaining({ sessionId: "a", content: expect.stringContaining("请继续"), images: [] }));
  expect(agentState.draft).toBe("尚未发送的草稿"); expect(agentState.images).toEqual([image]);
  await selectAgentSession("b"); await selectAgentSession("a");
  expect(agentState.draft).toBe("尚未发送的草稿"); expect(agentState.images).toEqual([image]);
});

/** 冷恢复请求是普通主聊天人类文本，不附加尚未发送的图片。 */
test("闲置子追问保留draft和images，失败安全可重试", async () => {
  agentState.draft = "保留草稿";
  agentState.children = [{ agentId: "child", parentSessionId: "a", delegationDepth: 1, description: "调查", status: "ready" }];
  vi.mocked(api.send).mockRejectedValue(new Error("token=secret"));
  vi.mocked(api.status).mockRejectedValue(new Error("internal-file"));
  await agentChildren.messageChild("child", "继续调查");
  expect(api.send).toHaveBeenCalledWith(expect.objectContaining({ sessionId: "a", content: expect.stringContaining("send_message"), images: [] }));
  expect(agentState.draft).toBe("保留草稿"); expect(agentState.images).toEqual([image]);
  expect(agentState.childrenError).toBe("发送子代理追问失败，请重试。");
  expect(agentState.error).not.toContain("secret");
});

/** 文件刷新等待期间若目标身份发生变化，不可把请求转送到新 root。 */
test("管理发送在flush后重新校验当前session", async () => {
  agentState.children = [{ agentId: "child", parentSessionId: "a", delegationDepth: 1, description: "调查", status: "ready" }];
  let finish!: () => void;
  vi.mocked(notePersistence.flushAll).mockReturnValueOnce(new Promise<void>(resolve => { finish = resolve; }));
  const pending = agentChildren.messageChild("child", "继续");
  agentChildren.reset(); agentState.session = session("b"); finish(); await pending;
  expect(api.send).not.toHaveBeenCalled(); expect(agentState.images).toEqual([image]);
});

/** 后端登记迟到也不能覆盖用户已经切换到的新 root 及其草稿。 */
test("迟到发送结果不污染新的root", async () => {
  let finish!: (run: AgentRun) => void;
  vi.mocked(api.send).mockReturnValueOnce(new Promise<AgentRun>(resolve => { finish = resolve; }));
  const pending = sendAgentMessage("旧消息");
  await vi.waitFor(() => { expect(api.send).toHaveBeenCalledTimes(1); });
  agentChildren.reset(); agentState.session = session("b"); agentState.draft = "新草稿";
  finish({ id: "late", sessionId: "a", state: "completed" } as AgentRun); await pending;
  expect(agentState.session.id).toBe("b"); expect(agentState.run).toBeNull();
  expect(agentState.draft).toBe("新草稿"); expect(agentState.images).toEqual([image]);
});

/** 不匹配会话的任务回包不能被视为成功，也不得清空待发附件。 */
test("发送结果session不匹配时保留草稿", async () => {
  agentState.draft = "仍待发送";
  vi.mocked(api.send).mockResolvedValue({ id: "wrong", sessionId: "b", state: "completed" } as AgentRun);
  await sendAgentMessage();
  expect(agentState.run).toBeNull(); expect(agentState.draft).toBe("仍待发送"); expect(agentState.images).toEqual([image]);
});

/** 尚未登记模型任务时的停止也必须阻止迟到 flush 启动新任务。 */
test("编辑器flush期间停止不会在flush完成后启动任务", async () => {
  let finish!: () => void;
  vi.mocked(notePersistence.flushAll).mockReturnValueOnce(new Promise<void>(resolve => { finish = resolve; }));
  const pending = sendAgentMessage("不要迟到执行");
  await stopAgent(); finish(); await pending;
  expect(api.send).not.toHaveBeenCalled(); expect(agentState.sending).toBe(false);
  expect(agentState.images).toEqual([image]);
});

/** 切库等待复习收尾期间，旧发送的 flush 恢复也不能启动新任务。 */
test("离库与发送准备交错不会遗留后台任务", async () => {
  let finishFlush!: () => void, finishReviews!: () => void;
  vi.mocked(notePersistence.flushAll).mockReturnValueOnce(new Promise<void>(resolve => { finishFlush = resolve; }));
  vi.mocked(settleAgentReviews).mockReturnValueOnce(new Promise<void>(resolve => { finishReviews = resolve; }));
  const sending = sendAgentMessage("旧库请求"), leaving = leaveAgentVault();
  finishFlush(); await sending;
  expect(api.send).not.toHaveBeenCalled();
  finishReviews(); await leaving;
  expect(agentState.session).toBeNull(); expect(agentState.run).toBeNull(); expect(agentState.sending).toBe(false);
});
