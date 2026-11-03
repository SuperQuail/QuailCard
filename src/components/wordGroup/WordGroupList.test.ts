import { describe, expect, test } from "vitest";
import { mount } from "@vue/test-utils";
import WordGroupList from "./WordGroupList.vue";
import type { WordEntry } from "../../domain/wordGroup";

/** 构造一页测试数据；默认完整，草稿按需覆盖。 */
function entry(overrides: Partial<WordEntry> = {}): WordEntry {
  return { id: "page-1", word: "ephemeral", phonetic: "", meaning: "短暂的", example: "", ...overrides };
}

const pages = [
  entry({ id: "p1", word: "ephemeral", meaning: "短暂的" }),
  entry({ id: "p2", word: "frugal", meaning: "" }),
  entry({ id: "p3", word: "", meaning: "" }),
];

describe("单词组列表", () => {
  test("展示进度与状态，点击回传原始页码", async () => {
    const wrapper = mount(WordGroupList, { props: { pages, current: 0 } });
    expect(wrapper.text()).toContain("ephemeral");
    expect(wrapper.get('[aria-label="第 3 页"]').text()).toContain("未填写单词");
    await wrapper.get('[aria-label="第 2 页"]').trigger("click");
    expect(wrapper.emitted("select")).toEqual([[1]]);
    // 当前页高亮用 aria-current 表达，便于测试与读屏。
    expect(wrapper.get('[aria-label="第 1 页"]').attributes("aria-current")).toBe("true");
    wrapper.unmount();
  });

  test("搜索与只看未完成只影响展示，页码仍是原始下标", async () => {
    const wrapper = mount(WordGroupList, { props: { pages, current: 1 } });
    await wrapper.get('input[aria-label="搜索单词"]').setValue("frugal");
    expect(wrapper.text()).toContain("frugal");
    expect(wrapper.text()).not.toContain("ephemeral");
    await wrapper.get('[aria-label="第 2 页"]').trigger("click");
    expect(wrapper.emitted("select")).toEqual([[1]]);

    await wrapper.get('input[aria-label="搜索单词"]').setValue("");
    await wrapper.get('[aria-label="只看未完成"]').trigger("click");
    expect(wrapper.get('[aria-label="只看未完成"]').attributes("aria-pressed")).toBe("true");
    // 完整页被过滤掉，草稿页仍在。
    expect(wrapper.text()).not.toContain("ephemeral");
    expect(wrapper.text()).toContain("frugal");
    wrapper.unmount();
  });

  test("底部添加按钮上抛 add，不可添加时禁用", async () => {
    const wrapper = mount(WordGroupList, { props: { pages, current: 0, canAdd: true } });
    const add = () => wrapper.findAll("button").find((button) => button.text().includes("添加单词"))!;
    await add().trigger("click");
    expect(wrapper.emitted("add")).toHaveLength(1);
    await wrapper.setProps({ canAdd: false });
    expect(add().attributes("disabled")).toBeDefined();
    wrapper.unmount();
  });

  test("空组与无匹配分别提示", async () => {
    const empty = mount(WordGroupList, { props: { pages: [], current: 0 } });
    expect(empty.text()).toContain("还没有单词");
    empty.unmount();
    const none = mount(WordGroupList, { props: { pages, current: 0 } });
    await none.get('input[aria-label="搜索单词"]').setValue("不存在");
    expect(none.text()).toContain("没有匹配的单词");
    none.unmount();
  });
});
