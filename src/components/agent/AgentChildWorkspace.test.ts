import { afterEach, expect, test, vi } from "vitest";
import { mount } from "@vue/test-utils";
import type { AgentChildInfo, AgentRun, AgentSession } from "../../domain/agent";
import AgentWorkspace from "./AgentWorkspace.vue";
import AgentChildDetail from "./AgentChildDetail.vue";
import AgentComposer from "./AgentComposer.vue";
import AgentPlan from "./AgentPlan.vue";
import AgentSafeToolCalls from "./AgentSafeToolCalls.vue";
/** 根会话对象用于检查查看子任务不会改动它的消息和草稿。 */
function session(id = "root"): AgentSession { return { id, title: id, formatVersion: 1, updatedAt: 0, messages: [], summary: "", selectedPaths: [] }; }
/** 独立运行夹具同时覆盖父与子实时更新。 */
function run(id = "root"): AgentRun { return { id: "r-" + id, sessionId: id, state: "running", sequence: 1, text: "", phase: "工作中", pendingWrite: null, pendingWriteId: null, error: null, reasoning: "", reasoningMessageId: "" }; }
/** 显式传入全部依赖，测试不访问后端服务。 */
function props() { return { session: session(), sessions: [], run: run(), draft: "父草稿保持", selectedPaths: [], providerId: "", providers: [], notes: [], memory: "", error: "", loading: false, sending: false, aiGrading: false, reviewFlow: vi.fn(), children: [{ agentId: "c", parentSessionId: "root", delegationDepth: 1, description: "运行中子任务", status: "running" }] as AgentChildInfo[] }; }
/** 测试结束恢复帧调度，不影响其他组件测试。 */
afterEach(() => vi.unstubAllGlobals());

// 子查看贯穿树、chip、工作区，但不采用主会话选择或发送事件。
test("父运行中子名称和计划均可打开，侧栏实时更新不改根输入", async () => {
  const input = props(); const snapshot = JSON.stringify(input);
  const wrapper = mount(AgentWorkspace, { props: input });
  await wrapper.get('[data-chip="children"]').trigger("click");
  await wrapper.get('[data-agent-id="c"] button.description').trigger("click");
  expect(wrapper.emitted("openChild")).toEqual([["c"]]);
  expect(wrapper.find('[aria-label="子代理"]').exists()).toBe(false);
  await wrapper.setProps({ childDetail: { id: "c", session: session("c"), run: { ...run("c"), text: "子实时一" }, loading: false, error: "" } });
  expect(wrapper.getComponent(AgentChildDetail).text()).toContain("子实时一");
  await wrapper.setProps({ childDetail: { id: "c", session: session("c"), run: { ...run("c"), text: "子实时二", sequence: 2 }, loading: false, error: "" } });
  expect(wrapper.getComponent(AgentChildDetail).text()).toContain("子实时二");
  expect(wrapper.getComponent(AgentComposer).props("draft")).toBe("父草稿保持");
  expect(wrapper.getComponent(AgentComposer).props("running")).toBe(true);
  expect(JSON.stringify(input)).toBe(snapshot);
  const detail = wrapper.getComponent(AgentChildDetail);
  detail.vm.$emit("refreshChild"); detail.vm.$emit("closeChild"); detail.vm.$emit("interruptChild", "c"); detail.vm.$emit("messageChild", "c", "子消息");
  expect(wrapper.emitted("refreshChild")).toEqual([[]]); expect(wrapper.emitted("closeChild")).toEqual([[]]);
  expect(wrapper.emitted("interruptChild")).toEqual([["c"]]); expect(wrapper.emitted("messageChild")).toEqual([["c", "子消息"]]);
  for (const event of ["session", "send", "draft", "stop", "action", "adopt"]) expect(wrapper.emitted(event)).toBeUndefined();
  expect(input.reviewFlow).not.toHaveBeenCalled();
  await wrapper.setProps({ run: null, session: { ...input.session, plan: { ownerSessionId: "root", revision: 1, steps: [{ id: "p", content: "委派", status: "completed", required: true, dependencies: [], childAgentId: "c", resultRefs: [] }] } } });
  await wrapper.get(".plan-head").trigger("click"); await wrapper.get(".child-link").trigger("click");
  expect(wrapper.emitted("openChild")).toEqual([["c"], ["c"]]);
  expect(wrapper.getComponent(AgentChildDetail).text()).toContain("子实时二");
  wrapper.unmount();
});

// 目标与计划仅在历史变化时重建，不跟随每个运行快照分配新对象。
test("实时快照保留静态计划引用", async () => {
  const input = props();
  const wrapper = mount(AgentWorkspace, { props: { ...input, session: { ...input.session, plan: { ownerSessionId: "root", revision: 1, steps: [{ id: "p", content: "当前计划", status: "in_progress", required: true, dependencies: [], resultRefs: [], childAgentId: null }] } } } });
  const plan = wrapper.getComponent(AgentPlan).props("plan");
  await wrapper.setProps({ run: { ...run(), sequence: 2, phase: "新阶段" } });
  expect(wrapper.getComponent(AgentPlan).props("plan")).toBe(plan); wrapper.unmount();
});

// 首次读取失败也能进入空树刷新，不把错误隐藏在不可达面板。
test("空子树错误提供刷新入口且管理错误进入侧栏", async () => {
  const wrapper = mount(AgentWorkspace, { props: { ...props(), children: [], childrenError: "读取失败" } });
  await wrapper.get('[data-chip="children"]').trigger("click");
  expect(wrapper.text()).toContain("读取失败"); await wrapper.get('[aria-label="刷新子任务"]').trigger("click");
  expect(wrapper.emitted("refreshChildren")).toEqual([[]]);
  await wrapper.setProps({ childDetail: { id: "c", session: null, run: null, loading: false, error: "" } });
  expect(wrapper.getComponent(AgentChildDetail).text()).toContain("读取失败"); wrapper.unmount();
});

// 父消息也只使用安全工具 DTO，不再调用旧 exchange 展开器。
test("父工具消息统一安全渲染且旧协议不泄漏", () => {
  const wrapper = mount(AgentWorkspace, { props: { ...props(), session: { ...session(), messages: [
    { id: "t", kind: "tool_calls", role: "assistant", content: "原始秘密", data: { rows: [{ id: "tool", name: "read_note", state: "ok", summary: "安全摘要" }] } },
    { id: "x", kind: "exchange", role: "assistant", content: "原始秘密", data: { exchange: "协议秘密" } },
  ] } } });
  expect(wrapper.findAllComponents(AgentSafeToolCalls)).toHaveLength(2);
  expect(wrapper.text()).toContain("安全摘要"); expect(wrapper.text()).not.toContain("秘密"); wrapper.unmount();
});

// 多次进度同帧只读一次高度，侧栏开着时父窗口不抢滚动，卸载取消悬挂帧。
test("父跟随滚动逐帧合并并隔离子详情", async () => {
  const frames: FrameRequestCallback[] = []; const cancel = vi.fn();
  vi.stubGlobal("requestAnimationFrame", vi.fn((callback: FrameRequestCallback) => { frames.push(callback); return frames.length; }));
  vi.stubGlobal("cancelAnimationFrame", cancel);
  const wrapper = mount(AgentWorkspace, { props: props() });
  const element = wrapper.get(".soft-scrollbar.min-h-0").element as HTMLElement;
  const height = vi.fn(() => 500); Object.defineProperty(element, "scrollHeight", { get: height });
  const scroll = vi.fn(); element.scrollTo = scroll;
  await wrapper.setProps({ run: { ...run(), sequence: 2 } }); await wrapper.setProps({ run: { ...run(), sequence: 3 } });
  // 滚动条浮层也会读 scrollHeight，这里只统计跟随滚动那一帧的读取次数。
  expect(frames).toHaveLength(1); height.mockClear(); frames[0](0); expect(height).toHaveBeenCalledTimes(1); expect(scroll).toHaveBeenCalledTimes(1);
  await wrapper.setProps({ childDetail: { id: "c", session: session("c"), run: run("c"), loading: false, error: "" }, run: { ...run(), sequence: 4 } });
  expect(frames).toHaveLength(1);
  await wrapper.setProps({ childDetail: null, run: { ...run(), sequence: 5 } });
  expect(frames).toHaveLength(2); wrapper.unmount(); expect(cancel).toHaveBeenCalledWith(2);
});
