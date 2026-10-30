import { mount } from "@vue/test-utils";
import { describe, expect, test } from "vitest";
import type { NoteSummary } from "../../domain/types";
import TagPanel from "./TagPanel.vue";

/** 构造标签面板夹具：tagsJson 用字符串保存，与后端 DTO 一致。 */
function note(path: string, title: string, tags: string[]): NoteSummary {
  return { path, title, tagsJson: JSON.stringify(tags), aliases: [], cardCount: 0, dueCount: 0, mtime: 0 };
}

const notes = [
  note("a.md", "A", ["工作/项目"]),
  note("b.md", "B", ["工作/项目/子"]),
  note("c.md", "C", ["生活"]),
];

describe("标签面板", () => {
  test("按名称排序显示标签树与计数，子标签默认折叠", () => {
    const wrapper = mount(TagPanel, { props: { notes } });
    const rows = wrapper.findAll("[data-tag]");
    expect(rows.map((row) => row.attributes("data-tag"))).toEqual(["工作", "生活"]);
    expect(rows[0]!.text()).toContain("2");
    expect(rows[0]!.attributes("aria-expanded")).toBe("false");
    wrapper.unmount();
  });

  test("点击标签展开子级并列出匹配笔记，父标签包含子标签", async () => {
    const wrapper = mount(TagPanel, { props: { notes } });
    await wrapper.get('[data-tag="工作"]').trigger("click");
    expect(wrapper.get('[data-tag="工作"]').attributes("aria-expanded")).toBe("true");
    expect(wrapper.findAll("[data-tag]").map((row) => row.attributes("data-tag")))
      .toEqual(["工作", "工作/项目", "生活"]);
    expect(wrapper.text()).toContain("匹配笔记（2）");
    const noteButtons = wrapper.findAll("[data-note-path]");
    expect(noteButtons.map((button) => button.attributes("data-note-path"))).toEqual(["a.md", "b.md"]);
    await noteButtons[0]!.trigger("click");
    expect(wrapper.emitted("select-note")).toEqual([["a.md"]]);
    wrapper.unmount();
  });

  test("再次点击同一标签收起子级，深层标签可继续展开", async () => {
    const wrapper = mount(TagPanel, { props: { notes } });
    await wrapper.get('[data-tag="工作"]').trigger("click");
    await wrapper.get('[data-tag="工作/项目"]').trigger("click");
    expect(wrapper.find('[data-tag="工作/项目/子"]').exists()).toBe(true);
    await wrapper.get('[data-tag="工作"]').trigger("click");
    expect(wrapper.get('[data-tag="工作"]').attributes("aria-expanded")).toBe("false");
    expect(wrapper.find('[data-tag="工作/项目"]').exists()).toBe(false);
    wrapper.unmount();
  });

  test("顶部过滤输入按完整标签路径筛选", async () => {
    const wrapper = mount(TagPanel, { props: { notes } });
    const input = wrapper.get('input[type="search"]');
    await input.setValue("项目");
    const rows = wrapper.findAll("[data-tag]");
    expect(rows.map((row) => row.attributes("data-tag"))).toEqual(["工作", "工作/项目", "工作/项目/子"]);
    await input.setValue("生活");
    expect(wrapper.findAll("[data-tag]").map((row) => row.attributes("data-tag"))).toEqual(["生活"]);
    wrapper.unmount();
  });

  test("没有标签与没有匹配结果给出不同空状态", async () => {
    const empty = mount(TagPanel, { props: { notes: [] } });
    expect(empty.text()).toContain("还没有标签");
    empty.unmount();
    const wrapper = mount(TagPanel, { props: { notes } });
    await wrapper.get('input[type="search"]').setValue("不存在");
    expect(wrapper.text()).toContain("没有匹配的标签");
    wrapper.unmount();
  });
});
