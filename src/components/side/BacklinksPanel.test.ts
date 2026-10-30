import { mount } from "@vue/test-utils";
import { describe, expect, test } from "vitest";
import type { Backlink } from "../../domain/types";
import BacklinksPanel from "./BacklinksPanel.vue";

/** 反链夹具：来源标题、行号与片段都是后端 DTO 字段。 */
const backlinks: Backlink[] = [
  { sourcePath: "a.md", sourceTitle: "A", line: 3, snippet: "指向 [[当前笔记]] 的一行" },
  { sourcePath: "dir/b.md", sourceTitle: "B", line: 12, snippet: "另一处引用" },
];

describe("反链面板", () => {
  test("显示标题、行号与片段，点击回传路径与行号", async () => {
    const wrapper = mount(BacklinksPanel, { props: { backlinks } });
    expect(wrapper.text()).toContain("反向链接（2）");
    expect(wrapper.text()).toContain("A");
    expect(wrapper.text()).toContain("L3");
    expect(wrapper.text()).toContain("指向");
    await wrapper.get('[data-backlink="a.md:3"]').trigger("click");
    expect(wrapper.emitted("open")).toEqual([["a.md", 3]]);
    await wrapper.get('[data-backlink="dir/b.md:12"]').trigger("click");
    expect(wrapper.emitted("open")).toEqual([["a.md", 3], ["dir/b.md", 12]]);
    wrapper.unmount();
  });

  test("没有反链时显示空状态且不渲染行", () => {
    const wrapper = mount(BacklinksPanel, { props: { backlinks: [] } });
    expect(wrapper.text()).toContain("还没有笔记引用它");
    expect(wrapper.findAll("[data-backlink]")).toHaveLength(0);
    expect(wrapper.emitted("open")).toBeUndefined();
    wrapper.unmount();
  });
});
