import { expect, test } from "vitest";
import { mount } from "@vue/test-utils";
import type { Goal } from "../../domain/agent";
import AgentGoalPanel from "./AgentGoalPanel.vue";
import AgentPlan from "./AgentPlan.vue";
/** 使用明确宿主事实，避免测试依赖全量状态服务。 */
function goal(): Goal {
  return { id: "g", revision: 1, objective: "整理本章", acceptanceCriteria: ["检查来源"], phase: "active", roundsStarted: 2, maxGoalRounds: 16, evidence: [], blocker: null };
}
/** 面板现在是下拉内容：计划已移出，当前步骤只来自宿主运行阶段。 */
function props() { return { goal: goal(), goalPhase: "active", runState: null, waitingReason: null, currentPhase: "正在读取笔记", busy: false, canResume: true }; }

// 同一份计划不再同时出现在头部面板与正文，避免重复渲染。
test("目标面板不再内嵌计划清单", () => {
  const wrapper = mount(AgentGoalPanel, { props: props() });
  expect(wrapper.findComponent(AgentPlan).exists()).toBe(false);
});

// 历史打开不提供续轮许可，组件不能擅自执行。
test("已存 active 目标等待用户继续且展示轮数与当前步骤", async () => {
  const wrapper = mount(AgentGoalPanel, { props: props() });
  expect(wrapper.text()).toContain("等待你");
  expect(wrapper.text()).toContain("等待继续 · 目标未完成");
  expect(wrapper.text()).toContain("已启动 2 轮");
  expect(wrapper.text()).toContain("当前步骤：正在读取笔记");
  expect(wrapper.emitted("resumeGoal")).toBeUndefined();
  await wrapper.findAll("button").find(button => button.text() === "继续目标")!.trigger("click");
  expect(wrapper.emitted("resumeGoal")).toEqual([[]]);
});

// 暂停和停止全部共同走宿主 stop，避免分叉成不一致的取消语义。
test("等待子任务仍非完成，暂停与停止全部使用同一事件", async () => {
  const wrapper = mount(AgentGoalPanel, { props: { ...props(), busy: true, runState: "running", waitingReason: "waitingChildren" } });
  expect(wrapper.text()).toContain("进行中");
  expect(wrapper.text()).toContain("等待子任务结果");
  expect(wrapper.text()).not.toContain("已完成");
  for (const label of ["暂停", "停止全部"]) await wrapper.findAll("button").find(button => button.text() === label)!.trigger("click");
  expect(wrapper.emitted("stop")).toEqual([[], []]);
});

// 等待用户、阻塞原因与成功终态分别呈现。
test("用户等待和额度阻塞保留原因，只有 complete 才显示目标完成", async () => {
  const wrapper = mount(AgentGoalPanel, { props: { ...props(), runState: "waiting", waitingReason: "waitingUser" } });
  expect(wrapper.text()).toContain("等待你确认或采纳");
  await wrapper.setProps({ waitingReason: null, goalPhase: "blocked", runState: "blocked", goal: { ...goal(), phase: "blocked", blocker: { reason: "轮数已用尽", attempts: [] } } });
  expect(wrapper.text()).toContain("受阻");
  expect(wrapper.text()).toContain("轮数已用尽");
  expect(wrapper.text()).not.toContain("已完成");
  await wrapper.setProps({ goalPhase: "complete", goal: { ...goal(), phase: "complete" }, runState: "completed" });
  expect(wrapper.text()).toContain("已完成");
  expect(wrapper.findAll("button").some(button => button.text() === "继续目标")).toBe(false);
});

// 即使旧目标没有条件，已有收据仍可展开安全查看来源与版本。
test("目标证据可展开完整安全来源信息", async () => {
  const wrapper = mount(AgentGoalPanel, { props: { ...props(), goal: { ...goal(), acceptanceCriteria: [], evidence: [{ criterionIndex: 2, goalRevision: 9, sourceVersion: "source-v3", receiptRef: '<a href="javascript:evil()">receipt</a>' }] } } });
  const details = wrapper.get(".agent-evidence"); expect(details.attributes("open")).toBeUndefined();
  (details.element as HTMLDetailsElement).open = true; await details.trigger("toggle");
  for (const value of ["criterionIndex", "2", "goalRevision", "9", "sourceVersion", "source-v3", "receiptRef", "receipt"]) expect(details.text()).toContain(value);
  expect(wrapper.find("a").exists()).toBe(false);
});

// 验收条件默认折叠，条数仍作为标题信息可见。
test("验收条件折叠时才铺开条目", async () => {
  const wrapper = mount(AgentGoalPanel, { props: props() });
  expect(wrapper.get("summary").text()).toContain("验收条件 1");
  expect(wrapper.get("details").attributes("open")).toBeUndefined();
  await wrapper.get("summary").trigger("click");
  expect(wrapper.text()).toContain("检查来源");
});

// 旧上限不参与展示或继续按钮判定，包括零默认值与已超过历史上限的目标。
test.each([0, 1, 16, 256, 4294967295])("旧 maxGoalRounds=%i 只保留兼容数据", async (maxGoalRounds) => {
  const wrapper = mount(AgentGoalPanel, { props: { ...props(), goal: { ...goal(), roundsStarted: 300, maxGoalRounds } } });
  expect(wrapper.get(".rounds").text()).toBe("已启动 300 轮");
  expect(wrapper.get(".rounds").text()).not.toContain("/");
  const resume = wrapper.findAll("button").find(button => button.text() === "继续目标")!;
  expect(resume.attributes("disabled")).toBeUndefined();
  await resume.trigger("click");
  expect(wrapper.emitted("resumeGoal")).toEqual([[]]);
});
