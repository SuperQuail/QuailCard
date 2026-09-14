import { describe, expect, test, vi } from "vitest";
import { mount } from "@vue/test-utils";
import AgentWorkspace from "./AgentWorkspace.vue";
import AgentText from "./AgentText.vue";
import Ribbon from "../Ribbon.vue";
import type { NoteSummary, ProviderSummary } from "../../domain/types";
/** 最小工作区输入保持组件测试与后端业务隔离。 */
function props() {
  return { session: null, sessions: [], run: null, draft: "学习", selectedPaths: [], providerId: "model", providers: [{ id: "model", name: "测试", model: "demo", hasCredential: true } as ProviderSummary], notes: [{ path: "a.md", title: "A" } as NoteSummary], memory: "", error: "", loading: false, sending: false, aiGrading: false, reviewFlow: vi.fn() };
}
describe("Agent 工作区入口与范围", () => {
  test("左侧机器人点击只发出打开事件", async () => {
    const wrapper = mount(Ribbon, { props: { treeOpen: true, dueCount: 0, dark: false } });
    await wrapper.get('[aria-label="打开学习 Agent"]').trigger("click"); expect(wrapper.emitted("open-agent")).toHaveLength(1);
  });
  test("指定笔记和发送是独立明确事件", async () => {
    const wrapper = mount(AgentWorkspace, { props: props() });
    const scope = wrapper.findAll("button").find(button => button.text().includes("整个知识库"))!;
    await scope.trigger("click"); await wrapper.get('input[type="checkbox"]').setValue(true);
    expect(wrapper.emitted("scope")?.[0]).toEqual([["a.md"]]);
    await wrapper.get('textarea[aria-label="给 Agent 发消息"]').trigger("keydown", { key: "Enter" });
    expect(wrapper.emitted("send")?.[0]).toEqual(["学习"]);
  });
  test("模型未配置时发送禁用并显示设置入口", () => {
    const wrapper = mount(AgentWorkspace, { props: { ...props(), providers: [] } });
    expect(wrapper.text()).toContain("先配置模型");
    expect(wrapper.findAll("button").find(button => button.text() === "发送")?.attributes("disabled")).toBeDefined();
  });
  test("聊天 HTML 按文字显示，笔记引用发出站内事件", async () => {
    const wrapper = mount(AgentText, { props: { text: '<img src=x onerror="evil()">\n\n[笔记](a.md)' } });
    expect(wrapper.find("img").exists()).toBe(false);
    await wrapper.get("button").trigger("click"); expect(wrapper.emitted("note")?.[0]).toEqual(["a.md"]);
  });
});
