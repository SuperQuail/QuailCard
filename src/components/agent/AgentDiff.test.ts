import { describe, expect, test } from "vitest";
import { mount } from "@vue/test-utils";
import AgentDiff from "./AgentDiff.vue";
import type { AgentChange } from "../../domain/agent";
import { serializeWordGroup, type WordEntry } from "../../domain/wordGroup";

/** 构造一条改动记录：差异面板只关心路径与前后内容。 */
function change(overrides: Partial<AgentChange>): AgentChange {
  return {
    formatVersion: 1,
    id: "change-1",
    path: "笔记.md",
    before: null,
    after: "",
    beforeHash: null,
    afterHash: "hash",
    state: "applied",
    ...overrides,
  };
}

/** 构造一份单词组文本，便于断言页级差异。 */
function documentOf(words: WordEntry[]): string {
  return serializeWordGroup({ formatVersion: 1, words });
}

describe("Agent 差异面板", () => {
  test("单词组按页展示改动，按钮改成打开单词组", () => {
    const before = documentOf([{ id: "p1", word: "ephemeral", phonetic: "", meaning: "短暂的", example: "" }]);
    const after = documentOf([{ id: "p1", word: "ephemeral", phonetic: "", meaning: "转瞬即逝的", example: "" }]);
    const wrapper = mount(AgentDiff, { props: { change: change({ path: "英语/核心词.qword", before, after }), busy: false } });
    expect(wrapper.text()).toContain("修改");
    expect(wrapper.text()).toContain("转瞬即逝的");
    expect(wrapper.text()).toContain("打开单词组");
    // 结构化列表替代 JSON 文本 diff。
    expect(wrapper.find("pre").exists()).toBe(false);
    wrapper.unmount();
  });

  test("Markdown 改动保持文本 diff", () => {
    const wrapper = mount(AgentDiff, { props: { change: change({ path: "笔记.md", before: "旧内容", after: "新内容" }), busy: false } });
    expect(wrapper.find("pre").exists()).toBe(true);
    expect(wrapper.text()).toContain("打开笔记");
    wrapper.unmount();
  });

  test("单词组内容无法解析时回退到文本 diff", () => {
    const wrapper = mount(AgentDiff, { props: { change: change({ path: "英语/坏文件.qword", before: null, after: "不是 JSON" }), busy: false } });
    expect(wrapper.find("pre").exists()).toBe(true);
    expect(wrapper.text()).toContain("不是 JSON");
    wrapper.unmount();
  });
});
