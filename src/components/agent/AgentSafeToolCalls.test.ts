import { mount } from "@vue/test-utils";
import { expect, test } from "vitest";
import AgentSafeToolCalls from "./AgentSafeToolCalls.vue";
import type { AgentMessage } from "../../domain/agent";

/** 模拟后端的公开行；原始协议字段故意混入以验证展示边界。 */
function message(rows: unknown[]): AgentMessage {
  return { id: "m", role: "assistant", kind: "tool_calls", content: "秘密正文", data: { rows } };
}

test("失败默认展开错误码与已校验参数，不重复空泛摘要", () => {
  const wrapper = mount(AgentSafeToolCalls, { props: { message: message([{
    id: "shot", name: "video_shot", state: "error", summary: "无法读取该时间点的画面",
    errorCode: "VIDEO_FRAME_FAILED", details: [{ label: "截图时间", value: "1352 秒" }, { label: "任务", value: "本地任务" }],
    arguments: "秘密参数", output: "秘密结果",
  }]) } });
  expect(wrapper.get("details").attributes("open")).toBeDefined();
  expect(wrapper.get("code").text()).toBe("VIDEO_FRAME_FAILED");
  expect(wrapper.get("dl").text()).toContain("1352 秒");
  expect(wrapper.text().split("无法读取该时间点的画面")).toHaveLength(2);
  expect(wrapper.text()).not.toContain("秘密");
});

test("成功展示字幕段数，支持实时结果更新", async () => {
  const wrapper = mount(AgentSafeToolCalls, { props: { message: message([{
    id: "t", name: "video_transcript", state: "running", summary: "正在获取字幕",
  }]) } });
  expect(wrapper.get('[role="status"]').text()).toContain("结果会自动更新");
  await wrapper.setProps({ message: message([{
    id: "t", name: "video_transcript", state: "ok", summary: "已读取字幕 2478 段",
    details: [{ label: "字幕段数", value: "2478" }],
  }]) });
  expect(wrapper.text()).toContain("2478");
  expect(wrapper.find('[role="status"]').exists()).toBe(false);
  expect(wrapper.get("details").attributes("open")).toBeUndefined();
});

test("旧记录明确缺少详情，畸形或额外协议字段不渲染", () => {
  const wrapper = mount(AgentSafeToolCalls, { props: { message: message([{
    id: "a", name: "read_note", state: "ok", summary: "读取成功",
    details: [null, { label: 1, value: "秘密" }], errorCode: "<script>秘密</script>",
    assistant: "秘密协议", arguments: "秘密参数",
  }]) } });
  expect(wrapper.text()).toContain("未保存可展示的参数或结果详情");
  expect(wrapper.text()).not.toContain("秘密");
  expect(wrapper.find("code").exists()).toBe(false);
});

test("详情仅作为文本渲染并限制异常长度", () => {
  const wrapper = mount(AgentSafeToolCalls, { props: { message: message([{
    id: "a", name: "read_note", state: "ok", summary: "读取成功",
    details: [{ label: "结果", value: "<img src=x>" }, { label: "长字段", value: "x".repeat(1000) }],
  }]) } });
  expect(wrapper.find("img").exists()).toBe(false);
  expect(wrapper.findAll("dd")[1]?.text()).toHaveLength(300);
});
