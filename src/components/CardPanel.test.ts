import { describe, expect, test } from "vitest";
import { mount } from "@vue/test-utils";
import CardPanel from "./CardPanel.vue";
import type { NoteCard } from "../domain/types";

/** 构造卡片，只覆盖入口按钮关心的字段。 */
function card(kind: "vocabulary" | "qa", id: string): NoteCard {
  return {
    id,
    notePath: "英语/词汇.md",
    sourceRef: "",
    kind,
    front: "短暂的",
    back: "ephemeral",
    detail: "",
    example: "",
    aliases: [],
    rubricPoints: [],
    position: 0,
    schedulerPhase: "new",
    dueAt: 0,
    intervalDays: 0,
    totalReviews: 0,
    version: 1,
  };
}

describe("卡片面板入口", () => {
  test("有单词卡时可以进入打字背诵并上抛事件", async () => {
    const wrapper = mount(CardPanel, { props: { cards: [card("vocabulary", "c1")], activeCardId: null } });
    const typing = wrapper.findAll("button").find((item) => item.text().includes("打字背诵单词"));
    expect(typing).toBeTruthy();
    expect((typing?.element as HTMLButtonElement).disabled).toBe(false);
    await typing?.trigger("click");
    expect(wrapper.emitted("start-typing")).toEqual([[]]);
  });

  test("只有问答卡时按钮禁用，避免进入没有可打内容的练习", () => {
    const wrapper = mount(CardPanel, { props: { cards: [card("qa", "q1")], activeCardId: null } });
    const typing = wrapper.findAll("button").find((item) => item.text().includes("打字背诵单词"));
    expect((typing?.element as HTMLButtonElement).disabled).toBe(true);
  });
});
