import { beforeEach, expect, test, vi } from "vitest";
import type { AgentSession } from "../../domain/agent";

vi.mock("../../api/agent", () => ({ children: vi.fn().mockResolvedValue([]), deleteSession: vi.fn(), session: vi.fn() }));
vi.mock("./providerStore", () => ({ activeProviderId: { value: "model" } }));
vi.mock("./noteStore", () => ({ noteOperationBusy: { value: false }, notePersistence: {}, activeNotePath: { value: "" } }));
vi.mock("../agentFiles", () => ({ coordinateAgentWrite: vi.fn(), undoAgentChange: vi.fn() }));
vi.mock("../agentReview", () => ({ clearAgentReviews: vi.fn(), agentReviewsBusy: () => false, settleAgentReviews: vi.fn() }));
vi.mock("./cardStore", () => ({ loadActiveCards: vi.fn(), reloadActiveCards: vi.fn() }));
vi.mock("./reviewStore", () => ({ refreshStats: vi.fn() }));
import * as api from "../../api/agent";
import { agentState, deleteAgentSession } from "./agentStore";

/** 最小会话保留删除后恢复范围所需契约。 */
function session(id: string): AgentSession { return { id, title: id, selectedPaths: [id + ".md"], messages: [], summary: "", updatedAt: 1, formatVersion: 1 }; }

beforeEach(() => {
  vi.resetAllMocks();
  Object.assign(agentState, { loading: false, sending: false, run: null, session: session("a"), sessions: [session("a"), session("b")], draft: "草稿", selectedPaths: ["a.md"], error: "" });
  vi.mocked(api.deleteSession).mockResolvedValue();
  vi.mocked(api.session).mockImplementation(async id => session(id));
});

test("删除当前会话后切换最近记录并恢复范围", async () => {
  await deleteAgentSession("a");
  expect(agentState.sessions.map(item => item.id)).toEqual(["b"]);
  expect(agentState.session?.id).toBe("b");
  expect(agentState.selectedPaths).toEqual(["b.md"]);
  expect(agentState.loading).toBe(false);
});

test("删除其他记录不改变当前草稿", async () => {
  await deleteAgentSession("b");
  expect(agentState.session?.id).toBe("a");
  expect(agentState.draft).toBe("草稿");
  expect(api.session).not.toHaveBeenCalled();
});

test("最后一条删除后显示空状态，不自动生成新历史", async () => {
  agentState.sessions = [session("a")];
  await deleteAgentSession("a");
  expect(agentState.session).toBeNull();
  expect(agentState.sessions).toEqual([]);
  expect(agentState.draft).toBe("");
});

test("删除失败保留记录和草稿供重试", async () => {
  vi.mocked(api.deleteSession).mockRejectedValue(new Error("删除失败"));
  await deleteAgentSession("a");
  expect(agentState.sessions).toHaveLength(2);
  expect(agentState.session?.id).toBe("a");
  expect(agentState.draft).toBe("草稿");
  expect(agentState.error).not.toBe("");
});

test("发送登记期间不允许删除", async () => {
  agentState.sending = true;
  await deleteAgentSession("a");
  expect(api.deleteSession).not.toHaveBeenCalled();
});
