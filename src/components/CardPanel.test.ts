import { describe, expect, test } from "vitest";
import { mount } from "@vue/test-utils";
import CardPanel from "./CardPanel.vue";
import type { NoteCard } from "../domain/types";

/** 构造一张用于展示的卡片；调度字段只取默认值。 */
function card(overrides: Partial<NoteCard> = {}): NoteCard {
  return {
    id: "card-1",
    notePath: "英语/核心词.qword",
    sourceRef: "",
    source: null,
    kind: "vocabulary",
    front: "短暂的",
    back: "ephemeral",
    detail: "/ˈfemərəl/",
    example: "Fame is ephemeral.",
    aliases: [],
    rubricPoints: [],
    position: 0,
    schedulerPhase: "new",
    dueAt: 0,
    intervalDays: 0,
    totalReviews: 0,
    version: 0,
    ...overrides,
  };
}

describe("单词组卡片区", () => {
  test("只读展示：隐藏编辑、删除、AI 拆卡与手动添加，保留复习本组", async () => {
    const wrapper = mount(CardPanel, { props: { cards: [card()], activeCardId: null, wordGroup: true } });
    expect(wrapper.text()).toContain("单词 1");
    expect(wrapper.find('[title="编辑卡片"]').exists()).toBe(false);
    expect(wrapper.find('[title="删除卡片"]').exists()).toBe(false);
    expect(wrapper.text()).not.toContain("AI 拆卡");
    expect(wrapper.text()).not.toContain("手动添加");
    expect(wrapper.get("footer").text()).toContain("复习本组");
    await wrapper.get("footer button").trigger("click");
    expect(wrapper.emitted("start-review")).toHaveLength(1);
    wrapper.unmount();
  });

  test("空组提示还没有单词，不出现 AI 拆卡入口", () => {
    const wrapper = mount(CardPanel, { props: { cards: [], activeCardId: null, wordGroup: true } });
    expect(wrapper.text()).toContain("还没有单词");
    expect(wrapper.text()).toContain("填写单词与释义后自动生成卡片");
    expect(wrapper.text()).not.toContain("AI 拆卡");
    wrapper.unmount();
  });

  test("Markdown 笔记保持原有卡片操作", async () => {
    const wrapper = mount(CardPanel, { props: { cards: [card({ notePath: "英语/笔记.md" })], activeCardId: null } });
    expect(wrapper.text()).toContain("卡片 1");
    expect(wrapper.find('[title="编辑卡片"]').exists()).toBe(true);
    expect(wrapper.text()).toContain("AI 拆卡");
    expect(wrapper.text()).toContain("手动添加");
    expect(wrapper.text()).toContain("复习这篇笔记");
    await wrapper.get('[title="编辑卡片"]').trigger("click");
    expect(wrapper.emitted("edit-card")).toEqual([["card-1"]]);
    wrapper.unmount();
  });
});

describe("卡片面板入口", () => {
  test("有单词卡时可以进入打字背诵并上抛事件", async () => {
    const wrapper = mount(CardPanel, { props: { cards: [card({ id: "c1" })], activeCardId: null } });
    const typing = wrapper.findAll("button").find((item) => item.text().includes("打字背诵单词"));
    expect(typing).toBeTruthy();
    expect((typing?.element as HTMLButtonElement).disabled).toBe(false);
    await typing?.trigger("click");
    expect(wrapper.emitted("start-typing")).toEqual([[]]);
  });

  test("只有问答卡时按钮禁用，避免进入没有可打内容的练习", () => {
    const wrapper = mount(CardPanel, { props: { cards: [card({ id: "q1", kind: "qa" })], activeCardId: null } });
    const typing = wrapper.findAll("button").find((item) => item.text().includes("打字背诵单词"));
    expect((typing?.element as HTMLButtonElement).disabled).toBe(true);
  });
});
