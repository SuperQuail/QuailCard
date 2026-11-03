import { mount, flushPromises } from "@vue/test-utils";
import { afterEach, describe, expect, test, vi } from "vitest";
import WordGroupEditor from "./WordGroupEditor.vue";
import { serializeWordGroup, type WordEntry } from "../../domain/wordGroup";
import type { DictionaryEntry } from "../../domain/types";

/** 已挂载组件：测试后统一卸载，避免窗口键盘监听泄漏到下一个用例。 */
const mounted: ReturnType<typeof mount>[] = [];

afterEach(() => {
  for (const wrapper of mounted.splice(0)) wrapper.unmount();
  vi.useRealTimers();
});

/** 构造测试用文档内容：默认字段为空，只覆盖需要展示的字段。 */
function contentOf(words: Array<Partial<WordEntry>>): string {
  return serializeWordGroup({
    formatVersion: 1,
    words: words.map((word, index) => ({ id: `page-${index + 1}`, word: "", phonetic: "", meaning: "", example: "", ...word })),
  });
}

/** 挂载分页编辑器；attachTo body 让 focus 断言与窗口键盘事件贴近真实环境。 */
function mountEditor(content: string, extra: Record<string, unknown> = {}): ReturnType<typeof mount> {
  const wrapper = mount(WordGroupEditor, {
    props: { notePath: "英语/核心词.qword", panelOpen: false, content, tabs: [{ path: "英语/核心词.qword", title: "核心词" }], ...extra },
    attachTo: document.body,
  });
  mounted.push(wrapper);
  return wrapper;
}

/** 取最后一次上抛的 [路径, JSON]：断言只看最新一次保存。 */
function lastSave(wrapper: ReturnType<typeof mount>): [string, string] {
  const events = wrapper.emitted("save-content")!;
  return events[events.length - 1] as [string, string];
}

describe("单词组分页编辑", () => {
  test("展示当前页与总页数，按钮与下拉都能翻页", async () => {
    const wrapper = mountEditor(contentOf([{ word: "alpha", meaning: "甲" }, { word: "beta", meaning: "乙" }]));
    expect(wrapper.text()).toContain("第 1 / 2 页");
    expect(wrapper.get('[aria-label="上一页"]').attributes("disabled")).toBeDefined();
    await wrapper.get('[aria-label="下一页"]').trigger("click");
    expect(wrapper.get('input[aria-label="单词"]').element).toHaveProperty("value", "beta");
    expect(wrapper.text()).toContain("第 2 / 2 页");
    expect(wrapper.get('[aria-label="下一页"]').attributes("disabled")).toBeDefined();
    await wrapper.get('[aria-label="跳转页码"]').setValue("0");
    expect(wrapper.get('input[aria-label="单词"]').element).toHaveProperty("value", "alpha");
  });

  test("Alt+←/→ 与 Home/End 翻页，输入框内不拦截", async () => {
    const wrapper = mountEditor(contentOf([{ word: "alpha" }, { word: "beta" }, { word: "gamma" }]));
    window.dispatchEvent(new KeyboardEvent("keydown", { key: "ArrowRight", altKey: true }));
    await flushPromises();
    expect(wrapper.text()).toContain("第 2 / 3 页");
    window.dispatchEvent(new KeyboardEvent("keydown", { key: "End" }));
    await flushPromises();
    expect(wrapper.text()).toContain("第 3 / 3 页");
    window.dispatchEvent(new KeyboardEvent("keydown", { key: "Home" }));
    await flushPromises();
    expect(wrapper.text()).toContain("第 1 / 3 页");
    // 焦点在输入框时 Home/End 属于文本编辑，不改变页码。
    const wordInput = wrapper.get('input[aria-label="单词"]');
    (wordInput.element as HTMLInputElement).focus();
    wordInput.element.dispatchEvent(new KeyboardEvent("keydown", { key: "End", bubbles: true }));
    await flushPromises();
    expect(wrapper.text()).toContain("第 1 / 3 页");
  });

  test("编辑字段立即序列化整份 JSON 上抛", async () => {
    const wrapper = mountEditor(contentOf([{ word: "alpha" }]), { notePath: "英语/核心词.qword" });
    await wrapper.get('input[aria-label="单词"]').setValue("ephemeral");
    await wrapper.get('textarea[aria-label="释义"]').setValue("短暂的");
    const events = wrapper.emitted("save-content")!;
    const [path, raw] = events[events.length - 1] as [string, string];
    expect(path).toBe("英语/核心词.qword");
    const parsed = JSON.parse(raw) as { words: WordEntry[] };
    expect(parsed.words[0]).toEqual({ id: "page-1", word: "ephemeral", phonetic: "", meaning: "短暂的", example: "" });
  });

  test("添加单词追加草稿页并聚焦单词输入", async () => {
    const wrapper = mountEditor(contentOf([{ word: "alpha" }]));
    await wrapper.findAll("button").find((button) => button.text().includes("添加单词"))!.trigger("click");
    await nextTickAndFlush();
    const [, raw] = lastSave(wrapper);
    const parsed = JSON.parse(raw) as { words: WordEntry[] };
    expect(parsed.words).toHaveLength(2);
    expect(parsed.words[1].word).toBe("");
    expect(parsed.words[1].id).toMatch(/^[A-Za-z0-9_-]{1,64}$/);
    expect(document.activeElement).toBe(wrapper.get('input[aria-label="单词"]').element);
  });

  test("删除本页移除当前页并保留其余页面", async () => {
    const wrapper = mountEditor(contentOf([{ word: "alpha" }, { word: "beta" }]));
    await wrapper.get('[aria-label="下一页"]').trigger("click");
    await wrapper.findAll("button").find((button) => button.text().includes("删除本页"))!.trigger("click");
    await nextTickAndFlush();
    const [, raw] = lastSave(wrapper);
    const parsed = JSON.parse(raw) as { words: WordEntry[] };
    expect(parsed.words).toHaveLength(1);
    expect(parsed.words[0].word).toBe("alpha");
    expect(wrapper.text()).toContain("第 1 / 1 页");
  });

  test("空组显示还没有单词，添加后进入第一页", async () => {
    const wrapper = mountEditor(contentOf([]));
    expect(wrapper.text()).toContain("还没有单词");
    await wrapper.findAll("button").find((button) => button.text().includes("添加单词"))!.trigger("click");
    await nextTickAndFlush();
    expect(wrapper.find('input[aria-label="单词"]').exists()).toBe(true);
    const [, raw] = lastSave(wrapper);
    expect((JSON.parse(raw) as { words: WordEntry[] }).words).toHaveLength(1);
  });

  test("解析失败进入只读，不上抛任何保存", async () => {
    const wrapper = mountEditor("{ 坏掉的 json");
    expect(wrapper.get('[role="alert"]').text()).toContain("只读");
    expect(wrapper.find('input[aria-label="单词"]').exists()).toBe(false);
    for (const button of wrapper.findAll("button")) {
      if (button.text().includes("添加单词")) expect(button.attributes("disabled")).toBeDefined();
    }
    expect(wrapper.emitted("save-content")).toBeUndefined();
  });

  test("输入单词停 500ms 后自动填入归一化音标", async () => {
    vi.useFakeTimers();
    const lookupWord = vi.fn(async (word: string): Promise<DictionaryEntry> => ({
      word, phonetic: "i'femәrəl", translation: null, definition: null, pos: null, collins: null, oxford: null, bnc: null, frq: null, exchange: null,
    }));
    const wrapper = mountEditor(contentOf([{ word: "" }]), { lookupWord });
    await wrapper.get('input[aria-label="单词"]').setValue("ephemeral");
    await vi.advanceTimersByTimeAsync(500);
    await flushPromises();
    const [, raw] = lastSave(wrapper);
    const parsed = JSON.parse(raw) as { words: WordEntry[] };
    expect(parsed.words[0].word).toBe("ephemeral");
    expect(parsed.words[0].phonetic).toBe("/iˈfemərəl/");
  });

  test("复习本组上抛独立事件", async () => {
    const wrapper = mountEditor(contentOf([{ word: "alpha", meaning: "甲" }]));
    await wrapper.findAll("button").find((button) => button.text().includes("复习本组"))!.trigger("click");
    expect(wrapper.emitted("start-review")).toHaveLength(1);
  });
});

/** 等待新增页触发的 nextTick 与 focus 链路完成。 */
async function nextTickAndFlush(): Promise<void> {
  await flushPromises();
}