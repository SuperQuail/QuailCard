import { expect, test } from "vitest";
import { mount } from "@vue/test-utils";
import type { Plan, PlanStatus } from "../../domain/agent";
import AgentPlan from "./AgentPlan.vue";
/** 五态夹具验证取消和阻塞不会被折算成完成。 */
function plan(statuses: PlanStatus[]): Plan {
  return { ownerSessionId: "s", revision: 2, steps: statuses.map((status, i) => ({ id: String(i), content: "步骤" + i, status, required: true, dependencies: [], childAgentId: i === 1 ? "child" : null, resultRefs: i === 2 ? ["receipt-1"] : [] })) };
}

// 正文只占一行是默认态，明细必须由用户点开。
test("默认折叠只显示进度行与当前项，展开才铺开五态与结果依据", async () => {
  const wrapper = mount(AgentPlan, { props: { plan: plan(["completed", "in_progress", "pending", "blocked", "cancelled"]) } });
  expect(wrapper.get("button").attributes("aria-expanded")).toBe("false");
  expect(wrapper.text()).toContain("计划 1/5 · 步骤1");
  for (const status of ["待开始", "已完成", "受阻", "已取消"]) expect(wrapper.text()).not.toContain(status);
  await wrapper.get("button").trigger("click");
  expect(wrapper.get("button").attributes("aria-expanded")).toBe("true");
  for (const status of ["待开始", "进行中", "已完成", "受阻", "已取消"]) expect(wrapper.text()).toContain(status);
  expect(wrapper.text()).toContain("1 已完成");
  expect(wrapper.text()).toContain("receipt-1");
});

// 子身份仅发查看事件，证据必须精确匹配，不能从引用推断路由。
test("子计划可查看且依据展示真实来源或明确不可定位", async () => {
  const wrapper = mount(AgentPlan, { props: { plan: plan(["completed", "in_progress", "pending"]), criteria: ["验收依据"], evidence: [{ criterionIndex: 0, goalRevision: 7, sourceVersion: '<img src=x>version', receiptRef: "receipt-1" }] } });
  await wrapper.get(".plan-head").trigger("click"); await wrapper.get(".child-link").trigger("click");
  expect(wrapper.emitted("openChild")).toEqual([["child"]]); expect(wrapper.find("a").exists()).toBe(false);
  const evidence = wrapper.get(".agent-evidence"); expect(evidence.attributes("open")).toBeUndefined();
  (evidence.element as HTMLDetailsElement).open = true; await evidence.trigger("toggle");
  expect(evidence.text()).toContain("goalRevision"); expect(evidence.text()).toContain("7");
  expect(evidence.text()).toContain("sourceVersion"); expect(evidence.text()).toContain("验收依据");
  expect(wrapper.find("img").exists()).toBe(false);
  await wrapper.setProps({ evidence: [] }); expect(wrapper.text()).toContain("无可验证来源信息");
});

// 并行进行项只展示第一项，其余用尾标计数；尾标在省略元素之外，窄屏不会被截断。
test("并行进行项用尾标计数且尾标不被省略", () => {
  const wrapper = mount(AgentPlan, { props: { plan: plan(["in_progress", "in_progress", "in_progress", "pending"]) } });
  expect(wrapper.text()).toContain("计划 0/4 · 步骤0");
  const more = wrapper.get(".plan-more");
  expect(more.text()).toBe("+2");
  expect(more.element.closest(".plan-label")).toBeNull();
  expect(wrapper.get(".plan-label").element.textContent).toContain("步骤0");
});

// 没有进行项时当前项退化为第一个待开始；全部完成时不拼多余的尾巴。
test("折叠头选择当前项，全完成不拼尾巴", () => {
  const pendingFirst = mount(AgentPlan, { props: { plan: plan(["completed", "pending"]) } });
  expect(pendingFirst.text()).toContain("计划 1/2 · 步骤1");
  const allDone = mount(AgentPlan, { props: { plan: plan(["completed", "completed"]) } });
  expect(allDone.text()).toBe("计划 2/2");
});

// 兼容旧计划 text 字段，但未知载荷不能混入成功步骤。
test("旧计划也支持五态并过滤损坏步骤", async () => {
  const wrapper = mount(AgentPlan, { props: { message: { id: "old", role: "assistant", kind: "plan", content: "", data: { steps: [{ text: "旧取消", status: "cancelled" }, { text: "未知", status: "oops" }, null] } } } });
  expect(wrapper.text()).toContain("计划 0/1");
  await wrapper.get("button").trigger("click");
  expect(wrapper.text()).toContain("旧取消");
  expect(wrapper.text()).toContain("已取消");
  expect(wrapper.text()).not.toContain("未知");
});
