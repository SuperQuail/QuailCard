import { mount } from "@vue/test-utils";
import { expect, test } from "vitest";
import AiSplitDialog from "./AiSplitDialog.vue";
import { emptyAiSplitState } from "../services/aiSplitTypes";

test("停止生成和关闭发送不同事件，进度来自后端状态", async () => {
  const state = { ...emptyAiSplitState(), open: true, step: "running" as const, generatedCount: 2, phase: "lookup" as const };
  const wrapper = mount(AiSplitDialog, { props: { state, providerConfigured: true } });
  expect(wrapper.text()).toContain("正在查询词典");
  expect(wrapper.text()).toContain("已生成 2 张有效草稿");
  await wrapper.findAll("button").find((button) => button.text() === "停止生成")!.trigger("click");
  expect(wrapper.emitted("stop")).toHaveLength(1);
  expect(wrapper.emitted("close")).toBeUndefined();
  await wrapper.get('[aria-label="关闭"]').trigger("click");
  expect(wrapper.emitted("close")).toHaveLength(1);
  wrapper.unmount();
});

test("未勾选草稿和提交中都禁用采纳，警告可见", () => {
  const state = { ...emptyAiSplitState(), step: "drafts" as const, warnings: ["材料不足，只生成一张"], drafts: [{ draftId: "stable", fields: { front: "问题", back: "答案" }, source: null }] };
  const wrapper = mount(AiSplitDialog, { props: { state, providerConfigured: true } });
  const adopt = wrapper.findAll("button").find((button) => button.text().startsWith("采纳"))!;
  expect(adopt.attributes("disabled")).toBeDefined();
  expect(wrapper.text()).toContain("材料不足，只生成一张");
  wrapper.unmount();
});
