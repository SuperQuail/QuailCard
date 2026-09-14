import { expect, test, vi } from "vitest";
import { mount } from "@vue/test-utils";
import type { AgentChildInfo, AgentMessage, AgentSession } from "../../domain/agent";
import AgentWorkspace from "./AgentWorkspace.vue";
import AgentChildren from "./AgentChildren.vue";
import AgentHeaderChips from "./AgentHeaderChips.vue";
import AgentAutonomyNotice from "./AgentAutonomyNotice.vue";
import AgentPlan from "./AgentPlan.vue";
/** 主子使用不同持久身份，测试查看子记录不会替换根会话。 */
function session(id = "root", messages: AgentMessage[] = []): AgentSession {
  return { id, title: id, formatVersion: 1, updatedAt: 0, messages, summary: "", selectedPaths: [] };
}
/** 最小展示输入不引入真实模型和状态服务。 */
function props() {
  return { session: session(), sessions: [], run: null, draft: "根草稿", selectedPaths: [], providerId: "", providers: [], notes: [], memory: "", error: "", loading: false, sending: false, aiGrading: false, reviewFlow: vi.fn() };
}
/** 旧计划的历史正文不应与当前清单并列占据聊天区。 */
function oldPlan(id: string, text: string): AgentMessage { return { id, role: "assistant", kind: "plan", content: "", data: { steps: [{ text, status: "pending" }] } }; }

// 当前计划是唯一实时真相，历史仅在没有新字段时兜底。
test("工作区只展示当前计划，旧会话最多展示最后一条计划", async () => {
  const root = session("root", [oldPlan("p1", "旧计划一"), oldPlan("p2", "旧计划二")]);
  const wrapper = mount(AgentWorkspace, { props: { ...props(), session: root } });
  expect(wrapper.text()).not.toContain("旧计划一");
  expect(wrapper.text()).toContain("旧计划二");
  await wrapper.setProps({ session: { ...root, plan: { ownerSessionId: "root", revision: 3, steps: [{ id: "latest", content: "最新计划", status: "in_progress", required: true, dependencies: [], resultRefs: [], childAgentId: null }] } } });
  expect(wrapper.text()).not.toContain("旧计划二");
  expect(wrapper.text()).toContain("最新计划");
  expect(wrapper.findAllComponents(AgentPlan)).toHaveLength(1);
});

// 常驻面板被彻底移除：自主状态只以头部 chip 与正文一行折叠卡出现。
test("聊天正文不再有常驻 autonomy-panels 区", () => {
  const wrapper = mount(AgentWorkspace, { props: props() });
  expect(wrapper.find(".autonomy-panels").exists()).toBe(false);
  expect(wrapper.findComponent(AgentHeaderChips).exists()).toBe(true);
  expect(wrapper.find('[data-chip="goal"]').exists()).toBe(false);
  expect(wrapper.find('[data-chip="children"]').exists()).toBe(false);
  expect(wrapper.find(".agent-plan").exists()).toBe(false);
});

// 子交互只透传组件事件，不由展示层直接调用后端。
test("子树事件完整透传且不冒充普通发送或主会话切换", async () => {
  const child: AgentChildInfo = { agentId: "c", parentSessionId: "root", delegationDepth: 0, description: "子任务", status: "idle" };
  const wrapper = mount(AgentWorkspace, { props: { ...props(), session: session("root"), children: [child] } });
  expect(wrapper.findAllComponents(AgentChildren)).toHaveLength(0);
  await wrapper.get('[data-chip="children"]').trigger("click");
  const tree = wrapper.getComponent(AgentChildren);
  tree.vm.$emit("refreshChildren");
  tree.vm.$emit("interruptChild", "c"); tree.vm.$emit("messageChild", "c", "检查结果");
  expect(wrapper.emitted("refreshChildren")).toEqual([[]]);
  expect(wrapper.emitted("interruptChild")).toEqual([["c"]]);
  expect(wrapper.emitted("messageChild")).toEqual([["c", "检查结果"]]);
  expect(wrapper.emitted("session")).toBeUndefined();
  expect(wrapper.emitted("send")).toBeUndefined();
});

// 消息载荷里嵌入 JSON 与 HTML 时，折叠行不泄露载荷，展开仅显示安全选定字段。
test("子通知默认摘要、展开安全正文，自动续轮不刷提示词", async () => {
  const message: AgentMessage = { id: "notice", role: "user", kind: "agent_message", content: '原始JSON {"secret":"raw"}', data: { notification: { agentId: "child", kind: "completed", content: '<img src=x onerror="evil()">研究结果' } } };
  const wrapper = mount(AgentAutonomyNotice, { props: { message } });
  expect(wrapper.text()).toContain("子任务本轮结束");
  expect(wrapper.text()).not.toContain("原始JSON");
  expect(wrapper.text()).not.toContain("研究结果");
  await wrapper.get("button").trigger("click");
  expect(wrapper.text()).toContain("研究结果");
  expect(wrapper.find("img").exists()).toBe(false);
  expect(wrapper.text()).not.toContain("原始JSON");
  const round = mount(AgentAutonomyNotice, { props: { message: { ...message, kind: "goal_round", content: "冗长续轮提示词", data: { round: 3 } } } });
  expect(round.text()).toContain("第 3 轮");
  expect(round.text()).not.toContain("冗长续轮提示词");
});

