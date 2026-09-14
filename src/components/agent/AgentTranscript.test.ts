import { expect, test } from "vitest";
import { mount } from "@vue/test-utils";
import type { AgentMessage, AgentRun, AgentSession } from "../../domain/agent";
import AgentTranscript from "./AgentTranscript.vue";
import AgentSafeToolCalls from "./AgentSafeToolCalls.vue";
/** 消息只提供展示 DTO，秘密字段用于验证白名单边界。 */
function message(kind: string, content = "", data: AgentMessage["data"] = null): AgentMessage { return { id: kind, role: "assistant", kind, content, data }; }
/** 子历史不依赖任何复习或采纳服务。 */
function session(messages: AgentMessage[]): AgentSession { return { id: "c", title: "子任务", formatVersion: 1, updatedAt: 0, messages, summary: "", selectedPaths: [] }; }
/** 安全工具 DTO 只能含四个展示字段，附加协议数据不得渲染。 */
function toolMessage() { return message("tool_calls", "原始正文秘密", { rows: [{ id: "1", name: "read_note", state: "running", summary: '<img src=x>安全摘要', arguments: "秘密参数", output: "秘密结果" }, { id: "2", name: "bad", state: "unknown", summary: "坏行" }, null], exchange: "秘密协议" }); }

// 安全工具行只消费四字段，旧协议既不投影也不格式化。
test("安全工具行支持状态实时更新并屏蔽旧协议", async () => {
  const wrapper = mount(AgentSafeToolCalls, { props: { message: toolMessage() } });
  expect(wrapper.findAll("details")).toHaveLength(1); expect(wrapper.text()).toContain("运行中");
  expect(wrapper.text()).toContain("安全摘要"); expect(wrapper.find("img").exists()).toBe(false);
  for (const secret of ["秘密参数", "秘密结果", "秘密协议", "原始正文秘密", "坏行"]) expect(wrapper.text()).not.toContain(secret);
  await wrapper.setProps({ message: message("tool_calls", "", { rows: [{ id: "1", name: "read_note", state: "ok", summary: "读取成功" }, { id: "2", name: "write_note", state: "error", summary: "写入失败" }] }) });
  expect(wrapper.text()).toContain("成功"); expect(wrapper.text()).toContain("失败");
  await wrapper.setProps({ message: message("exchange", "秘密正文", { rows: toolMessage().data?.rows, exchange: "秘密协议" }) });
  expect(wrapper.text()).toContain("原始协议已隐藏"); expect(wrapper.findAll("details")).toHaveLength(0); expect(wrapper.text()).not.toContain("秘密");
});

// 子详情不挂载会执行写入或创建复习 flow 的组件。
test("子记录展示正文通知草稿和安全工具，不暴露写入入口", async () => {
  const wrapper = mount(AgentTranscript, { props: { id: "c", run: null, session: session([
    message("text", '<script>evil()</script>[笔记](a.md)'),
    message("review", "复习摘要"),
    message("drafts", "草稿说明", { cards: [{ fields: { front: "安全问题", back: "安全答案" } }], raw: "草稿秘密" }),
    toolMessage(), message("exchange", "旧协议秘密", { exchange: "协议秘密" }),
    message("agent_message", "原始通知秘密", { notification: { agentId: "grandchild", kind: "completed", content: "子报告结果" } }),
  ]) } });
  expect(wrapper.find("script").exists()).toBe(false); expect(wrapper.find("a").exists()).toBe(false);
  expect(wrapper.text()).toContain("安全问题"); expect(wrapper.text()).toContain("仅查看，不可采纳");
  expect(wrapper.text()).toContain("不启动复习");
  expect(wrapper.findAll('input, textarea').length).toBe(0);
  expect(wrapper.findAll("button").some(button => /采纳|打开这轮复习/.test(button.text()))).toBe(false);
  await wrapper.get(".autonomy-notice button").trigger("click");
  expect(wrapper.text()).toContain("子报告结果");
  for (const secret of ["原始通知秘密", "协议秘密", "草稿秘密"]) expect(wrapper.text()).not.toContain(secret);
});

// 相同消息身份只出现一次，子轮次释放后保留最新落盘记录。
test("实时正文同身份合并，历史更新后移除run仍保留结果", async () => {
  const run: AgentRun = { id: "r", sessionId: "c", state: "running", sequence: 1, textMessageId: "text", text: "完整实时正文", phase: "", pendingWrite: null, pendingWriteId: null, error: null, reasoning: "", reasoningMessageId: "" };
  const wrapper = mount(AgentTranscript, { props: { id: "c", run, session: session([message("text", "完整")]) } });
  expect(wrapper.findAll("article")).toHaveLength(1); expect(wrapper.text()).toContain("完整实时正文");
  await wrapper.setProps({ session: session([message("text", "最终落盘结果")]), run: null });
  expect(wrapper.findAll("article")).toHaveLength(1); expect(wrapper.text()).toContain("最终落盘结果");
});
