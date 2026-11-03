import { mount, flushPromises } from "@vue/test-utils";
import { afterEach, describe, expect, test, vi } from "vitest";
import WordPageForm from "./WordPageForm.vue";
import type { DictionaryEntry } from "../../domain/types";
import type { WordEntry } from "../../domain/wordGroup";

/** 构造词典返回：音标故意用需要归一化的写法，验证填入前会统一字符。 */
function dictionaryEntry(word: string, phonetic: string | null): DictionaryEntry {
  return { word, phonetic, translation: null, definition: null, pos: null, collins: null, oxford: null, bnc: null, frq: null, exchange: null };
}

/** 构造一页测试数据。 */
function entry(overrides: Partial<WordEntry> = {}): WordEntry {
  return { id: "page-1", word: "ephemeral", phonetic: "", meaning: "", example: "", ...overrides };
}

afterEach(() => vi.useRealTimers());

describe("单词页表单", () => {
  test("自动查询只填空音标，手填内容不被覆盖", async () => {
    vi.useFakeTimers();
    const lookupWord = vi.fn(async (word: string) => dictionaryEntry(word, "i'femәrəl"));
    const wrapper = mount(WordPageForm, { props: { entry: entry({ phonetic: "/manual/" }), lookupWord } });
    await wrapper.get('input[aria-label="单词"]').setValue("ephemeral");
    await vi.advanceTimersByTimeAsync(500);
    await flushPromises();
    expect(lookupWord).toHaveBeenCalledWith("ephemeral");
    // 单词输入本身会上抛更新，但任何一次上抛里音标都必须保持手填值。
    for (const [next] of wrapper.emitted("update:entry")! as [WordEntry][]) {
      expect(next.phonetic).toBe("/manual/");
    }
    wrapper.unmount();
  });

  test("查询音标按钮可以覆盖手填并归一化字符", async () => {
    const lookupWord = vi.fn(async (word: string) => dictionaryEntry(word, "i'femәrəl"));
    const wrapper = mount(WordPageForm, { props: { entry: entry({ phonetic: "/manual/" }), lookupWord } });
    await wrapper.findAll("button").find((button) => button.text().includes("查询音标"))!.trigger("click");
    await flushPromises();
    const events = wrapper.emitted("update:entry")!;
    expect((events[events.length - 1][0] as WordEntry).phonetic).toBe("/iˈfemərəl/");
    wrapper.unmount();
  });

  test("组合输入期间不查询，确认候选后补查", async () => {
    vi.useFakeTimers();
    const lookupWord = vi.fn(async (word: string) => dictionaryEntry(word, "/test/"));
    const wrapper = mount(WordPageForm, { props: { entry: entry({ word: "" }), lookupWord } });
    const input = wrapper.get('input[aria-label="单词"]');
    await input.trigger("compositionstart");
    await input.setValue("ephemeral");
    await vi.advanceTimersByTimeAsync(800);
    expect(lookupWord).not.toHaveBeenCalled();
    await input.trigger("compositionend");
    await vi.advanceTimersByTimeAsync(500);
    await flushPromises();
    expect(lookupWord).toHaveBeenCalledWith("ephemeral");
    wrapper.unmount();
  });

  test("未收录时给出可手填提示，不弹错误", async () => {
    const lookupWord = vi.fn(async () => null);
    const wrapper = mount(WordPageForm, { props: { entry: entry(), lookupWord } });
    await wrapper.findAll("button").find((button) => button.text().includes("查询音标"))!.trigger("click");
    await flushPromises();
    expect(wrapper.text()).toContain("词典未收录");
    expect(wrapper.emitted("update:entry")).toBeUndefined();
    wrapper.unmount();
  });

  test("完整页提示已生成卡片，缺字段提示填写要求", async () => {
    const draft = mount(WordPageForm, { props: { entry: entry({ meaning: "" }) } });
    expect(draft.text()).toContain("填写单词与释义后自动生成卡片");
    const complete = mount(WordPageForm, { props: { entry: entry({ meaning: "短暂的" }) } });
    expect(complete.text()).toContain("已生成卡片");
    draft.unmount();
    complete.unmount();
  });
});