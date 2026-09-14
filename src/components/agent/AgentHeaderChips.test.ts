import { expect, test, vi } from "vitest";
import { mount } from "@vue/test-utils";
import { nextTick } from "vue";
import type { AgentChildInfo, Goal } from "../../domain/agent";
import AgentHeaderChips from "./AgentHeaderChips.vue";
import AgentChildren from "./AgentChildren.vue";
/** 目标夹具只给宿主事实：阶段、轮数与验收条数都由外部提供。 */
function goal(overrides: Partial<Goal> = {}): Goal {
  return { id: "g", revision: 1, objective: "把这一章整理成可复习的卡片", acceptanceCriteria: ["来源可核对"], phase: "active", roundsStarted: 2, maxGoalRounds: 16, evidence: [], blocker: null, ...overrides };
}
/** 子代理夹具覆盖三种状态短词。 */
function children(): AgentChildInfo[] {
  return [{ agentId: "c1", parentSessionId: "root", delegationDepth: 0, description: "查词", status: "running" },
    { agentId: "c2", parentSessionId: "root", delegationDepth: 0, description: "分析", status: "ready" }];
}
/** 全部可选是契约：缺省必须安全，不渲染空 chip。 */
function props(overrides: Record<string, unknown> = {}) {
  return { goal: goal(), goalPhase: "active", runState: null, waitingReason: null, busy: true, disabled: false,
    canResume: true, children: [] as AgentChildInfo[], rootId: "root", currentPhase: "正在读取笔记", ...overrides };
}

// 0 个子代理与无目标都不占头部位置。
test("0 个子代理不渲染 chip，无目标也不渲染", () => {
  const wrapper = mount(AgentHeaderChips, { props: props() });
  expect(wrapper.find('[data-chip="children"]').exists()).toBe(false);
  expect(wrapper.find('[data-chip="goal"]').exists()).toBe(true);
  const empty = mount(AgentHeaderChips, { props: { ...props(), goal: null, children: [] } });
  expect(empty.find('[data-chip="goal"]').exists()).toBe(false);
  expect(empty.text()).toBe("");
});

// chip 是极短状态词，长句只能进 title，不能当标题。
test("状态用五态短词，长句只出现在 title", () => {
  const wrapper = mount(AgentHeaderChips, { props: props({ busy: true }) });
  expect(wrapper.get('[data-chip="goal"]').text()).toBe("目标 · 进行中");
  const waiting = mount(AgentHeaderChips, { props: props({ busy: false }) });
  expect(waiting.get('[data-chip="goal"]').text()).toBe("目标 · 等待你");
  expect(waiting.get('[data-chip="goal"]').attributes("title")).toContain("等待继续 · 目标未完成");
  const failed = mount(AgentHeaderChips, { props: props({ runState: "failed" }) });
  expect(failed.get('[data-chip="goal"]').text()).toBe("目标 · 本轮失败");
  const stopped = mount(AgentHeaderChips, { props: props({ runState: "cancelled" }) });
  expect(stopped.get('[data-chip="goal"]').text()).toBe("目标 · 已停止");
  const done = mount(AgentHeaderChips, { props: props({ goalPhase: "complete", runState: "completed" }) });
  expect(done.get('[data-chip="goal"]').text()).toBe("目标 · 已完成");
});

// 打开下拉看全文与按钮，Esc 与点击外部都要收起。
test("目标下拉开关、Esc 与点击外部关闭", async () => {
  const wrapper = mount(AgentHeaderChips, { props: props() });
  const chip = wrapper.get('[data-chip="goal"]');
  expect(chip.attributes("aria-expanded")).toBe("false");
  expect(wrapper.find('[role="dialog"]').exists()).toBe(false);
  await chip.trigger("click");
  expect(chip.attributes("aria-expanded")).toBe("true");
  const panel = wrapper.get('[role="dialog"]');
  expect(panel.attributes("id")).toBe(chip.attributes("aria-controls"));
  expect(panel.text()).toContain("把这一章整理成可复习的卡片");
  expect(panel.text()).toContain("已启动 2 轮");
  await chip.trigger("click");
  expect(wrapper.find('[role="dialog"]').exists()).toBe(false);
  await chip.trigger("click");
  document.dispatchEvent(new KeyboardEvent("keydown", { key: "Escape" }));
  await nextTick();
  expect(wrapper.find('[role="dialog"]').exists()).toBe(false);
  await chip.trigger("click");
  document.body.dispatchEvent(new MouseEvent("mousedown", { bubbles: true }));
  await nextTick();
  expect(wrapper.find('[role="dialog"]').exists()).toBe(false);
});

// 下拉里的按钮只发出事件，组件不自己执行动作。
test("目标下拉的继续与停止按钮透传事件", async () => {
  const wrapper = mount(AgentHeaderChips, { props: props({ busy: false }) });
  await wrapper.get('[data-chip="goal"]').trigger("click");
  await wrapper.findAll("button").find(button => button.text() === "继续目标")!.trigger("click");
  expect(wrapper.emitted("resumeGoal")).toEqual([[]]);
  const busy = mount(AgentHeaderChips, { props: props({ busy: true }) });
  await busy.get('[data-chip="goal"]').trigger("click");
  for (const label of ["暂停", "停止全部"]) await busy.findAll("button").find(button => button.text() === label)!.trigger("click");
  expect(busy.emitted("stop")).toEqual([[], []]);
  expect(busy.emitted("resumeGoal")).toBeUndefined();
});

// 子代理内容整体搬进下拉，事件仍由 chip 组件向上透传。
test("子代理下拉放任务树并透传子事件", async () => {
  const wrapper = mount(AgentHeaderChips, { props: props({ children: children() }) });
  const chip = wrapper.get('[data-chip="children"]');
  expect(chip.text()).toBe("2 个子代理");
  expect(wrapper.findAllComponents(AgentChildren)).toHaveLength(0);
  await chip.trigger("click");
  const tree = wrapper.getComponent(AgentChildren);
  expect(wrapper.get('[role="dialog"]').text()).toContain("查词");
  expect(wrapper.get('[role="dialog"]').text()).toContain("运行中");
  tree.vm.$emit("refreshChildren");
  tree.vm.$emit("interruptChild", "c1");
  tree.vm.$emit("messageChild", "c1", "再核对一次");
  expect(wrapper.emitted("refreshChildren")).toEqual([[]]);
  expect(wrapper.emitted("interruptChild")).toEqual([["c1"]]);
  expect(wrapper.emitted("messageChild")).toEqual([["c1", "再核对一次"]]);
});

// 同一个头部只允许一个浮层；关闭按钮与卸载清理同样要有出口。
test("下拉互斥、关闭按钮生效且卸载摘掉 document 监听", async () => {
  const wrapper = mount(AgentHeaderChips, { props: props({ children: children() }) });
  await wrapper.get('[data-chip="goal"]').trigger("click");
  await wrapper.get('[data-chip="children"]').trigger("click");
  expect(wrapper.findAll('[role="dialog"]')).toHaveLength(1);
  expect(wrapper.get('[data-chip="goal"]').attributes("aria-expanded")).toBe("false");
  await wrapper.get('[aria-label="关闭子代理面板"]').trigger("click");
  expect(wrapper.findAll('[role="dialog"]')).toHaveLength(0);
  const removed = vi.spyOn(Document.prototype, "removeEventListener");
  await wrapper.get('[data-chip="goal"]').trigger("click");
  wrapper.unmount();
  expect(removed.mock.calls.map(call => call[0])).toEqual(expect.arrayContaining(["mousedown", "keydown"]));
  removed.mockRestore();
});
