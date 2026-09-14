import { afterEach, expect, test, vi } from "vitest";
import { mount } from "@vue/test-utils";
import AgentText from "./AgentText.vue";
import AgentWorkspace from "./AgentWorkspace.vue";
import AgentReasoning from "./AgentReasoning.vue";
import PhoneticAudio from "./PhoneticAudio.vue";
import type { AgentRun, AgentSession } from "../../domain/agent";

/** 每例恢复时钟，避免动画定时器影响其他组件用例。 */
afterEach(() => vi.useRealTimers());

test("收到整段后逐字显示，后续增量不重播，停止立即补全", async () => {
  vi.useFakeTimers();
  const wrapper = mount(AgentText, { props: { text: "你😀好", animate: true } });
  expect(wrapper.text()).toBe("");
  await vi.advanceTimersByTimeAsync(18);
  expect(wrapper.text()).toBe("你");
  await vi.advanceTimersByTimeAsync(18);
  expect(wrapper.text()).toBe("你😀");
  await wrapper.setProps({ text: "你😀好世界" });
  await vi.advanceTimersByTimeAsync(18);
  expect(wrapper.text()).toBe("你😀好");
  await wrapper.setProps({ animate: false });
  expect(wrapper.text()).toBe("你😀好世界");
  wrapper.unmount();
  expect(vi.getTimerCount()).toBe(0);
});

test("音标单独成段时沿用上一段的单词生成朗读按钮", () => {
  const wrapper = mount(AgentText, { props: { text: "第1个单词: Ambition\n\n音标: /æmˈbɪʃən/" } });
  const audio = wrapper.findComponent(PhoneticAudio);
  expect(audio.exists()).toBe(true);
  expect(audio.props("word")).toBe("Ambition");
  wrapper.unmount();
});

test("实时思考折叠显示最后一行，展开后可见全文", async () => {
  const session: AgentSession = { id: "s", title: "对话", formatVersion: 1, updatedAt: 0, messages: [], summary: "", selectedPaths: [] };
  const run: AgentRun = { id: "r", sessionId: "s", textMessageId: "m", state: "running", sequence: 1, text: "", phase: "思考与回答", pendingWrite: null, pendingWriteId: null, error: null, reasoning: "先读材料\n再拆卡片", reasoningMessageId: "r1" };
  const wrapper = mount(AgentWorkspace, { props: { session, sessions: [], run, draft: "", selectedPaths: [], providerId: "", providers: [], notes: [], memory: "", error: "", loading: false, sending: false, aiGrading: false, reviewFlow: vi.fn() } });
  const reasoning = wrapper.getComponent(AgentReasoning);
  expect(reasoning.text()).toContain("再拆卡片");
  expect(reasoning.text()).not.toContain("先读材料");
  await reasoning.get("button").trigger("click");
  expect(reasoning.text()).toContain("先读材料");
  wrapper.unmount();
});

test("流式消息转为最终历史时保持组件身份，尾字继续输出且不重复", async () => {
  vi.useFakeTimers();
  const session: AgentSession = { id: "s", title: "对话", formatVersion: 1, updatedAt: 0, messages: [], summary: "", selectedPaths: [] };
  const run: AgentRun = { id: "r", sessionId: "s", textMessageId: "m", state: "running", sequence: 1, text: "你好世界", phase: "回答", pendingWrite: null, pendingWriteId: null, error: null, reasoning: "", reasoningMessageId: "" };
  const wrapper = mount(AgentWorkspace, { props: { session, sessions: [], run, draft: "", selectedPaths: [], providerId: "", providers: [], notes: [], memory: "", error: "", loading: false, sending: false, aiGrading: false, reviewFlow: vi.fn() } });
  const text = wrapper.getComponent(AgentText);
  await vi.advanceTimersByTimeAsync(18);
  expect(text.text()).toBe("你");
  await wrapper.setProps({ session: { ...session, messages: [{ id: "m", role: "assistant", kind: "text", content: "你好世界", data: null }] }, run: { ...run, state: "completed", text: "", sequence: 2 } });
  expect(wrapper.findAllComponents(AgentText)).toHaveLength(1);
  expect(wrapper.getComponent(AgentText).element).toBe(text.element);
  expect(text.text()).toBe("你");
  await vi.advanceTimersByTimeAsync(60);
  expect(text.text()).toBe("你好世界");
  wrapper.unmount();
});
