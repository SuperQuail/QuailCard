import { expect, test } from "vitest";
import { mount } from "@vue/test-utils";
import type { AgentChildInfo, AgentRun, AgentSession } from "../../domain/agent";
import AgentChildDetail from "./AgentChildDetail.vue";
/** 独立的子身份夹具，避免测试依赖父 store。 */
function session(): AgentSession { return { id: "child", title: "历史标题", formatVersion: 1, updatedAt: 0, messages: [{ id: "m", role: "assistant", kind: "text", content: "历史结果", data: null }], summary: "", selectedPaths: [] }; }
/** 运行与历史显式独立，测试实时未落盘阶段。 */
function run(text = "实时进展"): AgentRun { return { id: "r", sessionId: "child", state: "running", sequence: 1, text, phase: "检查来源", pendingWrite: null, pendingWriteId: null, error: null, reasoning: "", reasoningMessageId: "" }; }
/** 直属关系来自 children，而非历史文件声明。 */
function props() { return { id: "child", rootId: "root", session: null as AgentSession | null, run: null as AgentRun | null, loading: false, error: "", children: [{ agentId: "child", parentSessionId: "root", delegationDepth: 1, description: "检查资料", status: "running" }] as AgentChildInfo[] }; }

// 加载与错误不会清除已有历史，丢失运行快照时明确回退。
test("详情覆盖空、加载、安全错误、运行与历史回退", async () => {
  const wrapper = mount(AgentChildDetail, { props: props() });
  expect(wrapper.text()).toContain("暂无可查看");
  await wrapper.setProps({ loading: true });
  expect(wrapper.text()).toContain("正在加载");
  await wrapper.setProps({ run: run() });
  expect(wrapper.text()).toContain("实时进展");
  await wrapper.setProps({ loading: false, error: '<img src=x onerror="evil()">读取失败', session: session() });
  expect(wrapper.text()).toContain("历史结果"); expect(wrapper.text()).toContain("读取失败");
  expect(wrapper.find("img").exists()).toBe(false);
  await wrapper.setProps({ run: run("新的实时内容") });
  expect(wrapper.text()).toContain("新的实时内容");
  await wrapper.setProps({ run: null, children: [] });
  expect(wrapper.text()).toContain("当前无实时轮次"); expect(wrapper.text()).toContain("历史结果");
  expect(wrapper.text()).toContain("状态不可用");
});

// 查看与消息操作不能变成主发送，关系更新后即时撤销追加入口。
test("运行中仍能刷新关闭，仅直属消息复用独立事件", async () => {
  const wrapper = mount(AgentChildDetail, { props: { ...props(), session: session(), run: run() } });
  await wrapper.get('[aria-label="刷新子任务详情"]').trigger("click");
  await wrapper.get('[aria-label="关闭子任务详情"]').trigger("click");
  await wrapper.findAll("button").find(button => button.text() === "中断本轮")!.trigger("click");
  await wrapper.get("textarea").setValue(" 子要求 "); await wrapper.get("form").trigger("submit");
  expect(wrapper.emitted("refreshChild")).toEqual([[]]); expect(wrapper.emitted("closeChild")).toEqual([[]]);
  expect(wrapper.emitted("interruptChild")).toEqual([["child"]]); expect(wrapper.emitted("messageChild")).toEqual([["child", "子要求"]]);
  for (const event of ["session", "send", "draft", "action", "adopt", "stop"]) expect(wrapper.emitted(event)).toBeUndefined();
  await wrapper.setProps({ children: [{ ...props().children[0], parentSessionId: "other", status: "ready" }] });
  expect(wrapper.find("textarea").exists()).toBe(false); expect(wrapper.text()).not.toContain("中断本轮");
  expect(wrapper.text()).toContain("可恢复"); expect(wrapper.text()).toContain("通过直接父任务协调");
});

// 返回父级只在已知关系内导航，管理错误在当前侧栏即可看见。
test("父级导航限制为当前根或已知子树并展示管理错误", async () => {
  const wrapper = mount(AgentChildDetail, { props: { ...props(), managementError: "追加失败，请重试" } });
  expect(wrapper.text()).toContain("追加失败，请重试");
  await wrapper.findAll("button").find(button => button.text() === "返回父任务")!.trigger("click");
  expect(wrapper.emitted("closeChild")).toEqual([[]]);
  const parent = { ...props().children[0], agentId: "parent" };
  await wrapper.setProps({ children: [parent, { ...props().children[0], parentSessionId: "parent" }] });
  await wrapper.findAll("button").find(button => button.text() === "返回父任务")!.trigger("click");
  expect(wrapper.emitted("openChild")).toEqual([["parent"]]);
  await wrapper.setProps({ children: [{ ...props().children[0], parentSessionId: "missing" }] });
  expect(wrapper.text()).toContain("直接父任务：missing");
  expect(wrapper.findAll("button").some(button => button.text() === "返回父任务")).toBe(false);
});

// 历史 parent 字段不是授权，错身份实时快照必须丢弃。
test("不采用历史关系授权且不渲染其他会话快照", async () => {
  const wrapper = mount(AgentChildDetail, { props: { ...props(), children: [], session: { ...session(), id: "wrong", parentSessionId: "root" }, run: { ...run(), sessionId: "root" } } });
  expect(wrapper.text()).not.toContain("历史结果"); expect(wrapper.text()).not.toContain("实时进展");
  expect(wrapper.find("form").exists()).toBe(false);
  await wrapper.setProps({ children: props().children });
  await wrapper.get("textarea").setValue("旧子任务草稿");
  await wrapper.setProps({ id: "other" }); await wrapper.setProps({ id: "child" });
  expect((wrapper.get("textarea").element as HTMLTextAreaElement).value).toBe("");
});
