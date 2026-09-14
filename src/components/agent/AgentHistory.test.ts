import { expect, test } from "vitest";
import { mount } from "@vue/test-utils";
import AgentHistory from "./AgentHistory.vue";
import type { AgentSession } from "../../domain/agent";

const sessions = [
  { id: "one", title: "数学笔记", updatedAt: 1 },
  { id: "two", title: "英语复习", updatedAt: 2 },
] as AgentSession[];

test("历史支持搜索、选择和确认后删除，删除按钮不会触发选择", async () => {
  const wrapper = mount(AgentHistory, { props: { sessions, activeId: "two", busy: false } });
  await wrapper.get('[aria-current="true"]').trigger("click");
  expect(wrapper.emitted("select")).toEqual([["two"]]);
  await wrapper.get("input").setValue("数学");
  expect(wrapper.text()).not.toContain("英语复习");
  await wrapper.get('[aria-label="删除会话：数学笔记"]').trigger("click");
  expect(wrapper.emitted("delete")).toBeUndefined();
  await wrapper.findAll("button").find(button => button.text() === "确认删除")!.trigger("click");
  expect(wrapper.emitted("delete")).toEqual([["one"]]);
  expect(wrapper.emitted("select")).toHaveLength(1);
});

test("操作中禁止切换与删除，仍可关闭面板", async () => {
  const wrapper = mount(AgentHistory, { props: { sessions, activeId: "two", busy: true } });
  expect(wrapper.get('[aria-current="true"]').attributes("disabled")).toBeDefined();
  await wrapper.get('[aria-label="删除会话：数学笔记"]').trigger("click");
  expect(wrapper.text()).not.toContain("确认删除");
  await wrapper.get('[aria-label="关闭历史记录"]').trigger("click");
  expect(wrapper.emitted("close")).toHaveLength(1);
});
