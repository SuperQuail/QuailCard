import { expect, test } from "vitest";
import { mount } from "@vue/test-utils";
import type { AgentChildInfo } from "../../domain/agent";
import AgentChildren from "./AgentChildren.vue";
import { childTreeRows } from "./agentChildTree";
/** 故意打乱顺序和声明深度，树必须依靠父身份还原。 */
function children(): AgentChildInfo[] {
  return [
    { agentId: "grandchild", parentSessionId: "child", delegationDepth: 0, description: "查词孙任务", status: "running" },
    { agentId: "sibling", parentSessionId: "root", delegationDepth: 8, description: "同级分析", status: "ready" },
    { agentId: "child", parentSessionId: "root", delegationDepth: 2, description: "分析任务", status: "idle" },
  ];
}
// 列表顺序与声明深度不是关系权威。
test("真实父关系缩进并仅为运行中任务提供单轮中断", async () => {
  const wrapper = mount(AgentChildren, { props: { children: children(), rootId: "root" } });
  const rows = wrapper.findAll("[data-agent-id]");
  expect(rows.map(row => [row.attributes("data-agent-id"), row.attributes("data-depth")])).toEqual([["sibling", "0"], ["child", "0"], ["grandchild", "1"]]);
  // 缩进写在 padding 上，层级差必须体现在行内边距里。
  expect(rows[0].attributes("style")).toContain("padding-inline-start: 12px");
  expect(rows[2].attributes("style")).toContain("padding-inline-start: 28px");
  const grandchild = wrapper.get('[data-agent-id="grandchild"]');
  // 孙级只留短词，完整解释进 title。
  expect(grandchild.text()).toContain("经父级");
  expect(grandchild.find('[title="追加要求请通过直接父任务协调"]').exists()).toBe(true);
  expect(grandchild.findAll("button").some(button => button.text() === "追加消息")).toBe(false);
  await grandchild.findAll("button").find(button => button.text() === "中断本轮")!.trigger("click");
  expect(wrapper.emitted("interruptChild")).toEqual([["grandchild"]]);
  expect(wrapper.get('[data-agent-id="child"]').text()).not.toContain("中断本轮");
});

// 行数不超窗口时全量渲染：小列表不该出现 spacer，行高仍是虚拟列表估算的 52px。
test("小列表全量渲染且行高固定为 52px", () => {
  const wrapper = mount(AgentChildren, { props: { children: children(), rootId: "root" } });
  const list = wrapper.get('[role="list"]');
  expect(list.attributes("data-virtual-start")).toBe("0");
  expect(list.attributes("data-virtual-end")).toBe("3");
  expect(wrapper.findAll('[data-virtual-spacer]')).toHaveLength(0);
  expect(wrapper.get('[data-agent-id="child"]').element.parentElement!.style.height).toBe("52px");
});

// 查看名称是独立按钮，即使父输入被锁定也不锁只读查看。
test("运行中子名称仍可查看且不发送消息", async () => {
  const wrapper = mount(AgentChildren, { props: { children: children(), rootId: "root", disabled: true } });
  await wrapper.get('[data-agent-id="grandchild"] button.description').trigger("click");
  expect(wrapper.emitted("openChild")).toEqual([["grandchild"]]);
  expect(wrapper.emitted("interruptChild")).toBeUndefined(); expect(wrapper.emitted("messageChild")).toBeUndefined();
  expect(wrapper.find("form").exists()).toBe(false);
});

// 面板栏只放摘要与刷新；追加表单固定在列表之外，不随列表滚动。
test("面板摘要与底部固定表单各就各位", async () => {
  const wrapper = mount(AgentChildren, { props: { children: children(), rootId: "root" } });
  expect(wrapper.get(".panel-bar").text()).toContain("3 个子代理 · 1 运行中");
  expect(wrapper.find("form").exists()).toBe(false);
  await wrapper.get('[data-agent-id="child"]').findAll("button").find(button => button.text() === "追加消息")!.trigger("click");
  const form = wrapper.get("form");
  expect(form.element.closest('[role="list"]')).toBeNull();
  expect(form.text()).toContain("追加到：分析任务");
  expect(form.get("textarea").attributes("aria-label")).toBe("给子任务追加消息");
});

// UI 保留坏记录供人工定位，而不是递归卡死或伪造其直属关系。
test("缺父与环只展示一次并标记关系异常", () => {
  const a = { ...children()[0], agentId: "a", parentSessionId: "b" };
  const b = { ...a, agentId: "b", parentSessionId: "a" };
  const orphan = { ...a, agentId: "orphan", parentSessionId: "missing" };
  const rows = childTreeRows([a, b, a, orphan], "root");
  expect(new Set(rows.map(row => row.child.agentId)).size).toBe(3);
  expect(rows).toHaveLength(3);
  expect(rows.filter(row => row.detached)).toHaveLength(2);
});

// 坏记录在描述位置显示父身份而不是伪装成正常子任务。
test("关系异常行在描述位置显示父身份", () => {
  const orphan = { agentId: "orphan", parentSessionId: "missing", delegationDepth: 0, description: "孤立任务", status: "idle" } as AgentChildInfo;
  const wrapper = mount(AgentChildren, { props: { children: [orphan], rootId: "root" } });
  const label = wrapper.get('[data-agent-id="orphan"] .description');
  expect(label.text()).toBe("关系异常：missing");
  expect(label.attributes("title")).toContain("孤立任务");
});

// 服务仍会复验，组件先阻止空白、超字符和超字节的意外提交。
test("追加拒绝空白、超16000字符与超16KiB，合法消息保留子身份", async () => {
  const wrapper = mount(AgentChildren, { props: { children: children(), rootId: "root" } });
  await wrapper.get('[data-agent-id="child"]').findAll("button").find(button => button.text() === "追加消息")!.trigger("click");
  const input = wrapper.get("textarea");
  await input.setValue("   "); await wrapper.get("form").trigger("submit");
  expect(wrapper.text()).toContain("不能只包含空白");
  await input.setValue("x".repeat(16001)); await wrapper.get("form").trigger("submit");
  expect(wrapper.text()).toContain("不能超过 16000 字符");
  await input.setValue("中".repeat(6000)); await wrapper.get("form").trigger("submit");
  expect(wrapper.text()).toContain("不能超过 16 KiB");
  expect(wrapper.emitted("messageChild")).toBeUndefined();
  await input.setValue(" 请检查引用 "); await wrapper.get("form").trigger("submit");
  expect(wrapper.emitted("messageChild")).toEqual([["child", "请检查引用"]]);
  // 发送成功后表单关闭并清空草稿，不会把上一条要求留给下一个目标。
  expect(wrapper.find("form").exists()).toBe(false);
});

// 错误与刷新入口不藏在折叠内容深处。
test("空树仍可刷新且安全显示错误", async () => {
  const wrapper = mount(AgentChildren, { props: { children: [], rootId: "root", error: "读取失败" } });
  expect(wrapper.get('[role="alert"]').text()).toBe("读取失败");
  expect(wrapper.text()).toContain("暂无子任务");
  await wrapper.get('[aria-label="刷新子任务"]').trigger("click");
  expect(wrapper.emitted("refreshChildren")).toEqual([[]]);
});

// 根身份切换等于换了会话：选中目标与草稿都必须丢弃。
test("切换根会话丢弃选中目标与草稿", async () => {
  const wrapper = mount(AgentChildren, { props: { children: children(), rootId: "root" } });
  await wrapper.get('[data-agent-id="child"]').findAll("button").find(button => button.text() === "追加消息")!.trigger("click");
  await wrapper.get("textarea").setValue("旧会话草稿");
  await wrapper.setProps({ rootId: "other" });
  expect(wrapper.find("form").exists()).toBe(false);
  expect(wrapper.text()).not.toContain("旧会话草稿");
});
