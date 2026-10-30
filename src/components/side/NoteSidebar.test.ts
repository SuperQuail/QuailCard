import { mount } from "@vue/test-utils";
import { beforeEach, describe, expect, test } from "vitest";
import NoteSidebar from "./NoteSidebar.vue";
import { leftSidebarTab } from "../../services/stores/uiStore";
import type { NoteSummary } from "../../domain/types";

/** 构造测试用笔记摘要。 */
function note(path: string): NoteSummary {
  return { path, title: path, tagsJson: "[]", aliases: [], cardCount: 0, dueCount: 0, mtime: 1 };
}

/** FileTree 与本测试无关，替换为桩以只验证栏位切换。 */
const stubs = { FileTree: true, TagPanel: true };

/** 挂载左栏并注入最小 props。 */
function mountSidebar() {
  return mount(NoteSidebar, {
    props: { notes: [note("a.md")], folderNames: [], activeNotePath: "a.md", dueCount: 0, pathChange: null },
    global: { stubs },
  });
}

beforeEach(() => {
  leftSidebarTab.value = "files";
});

describe("左栏页签", () => {
  test("默认显示文件树", () => {
    const wrapper = mountSidebar();
    expect(wrapper.find("file-tree-stub").exists()).toBe(true);
    expect(wrapper.find("tag-panel-stub").exists()).toBe(false);
    wrapper.unmount();
  });

  test("切到标签页签后显示标签面板，状态写回 store", async () => {
    const wrapper = mountSidebar();
    await wrapper.get('[role="tab"][aria-selected="false"]').trigger("click");
    expect(leftSidebarTab.value).toBe("tags");
    expect(wrapper.find("tag-panel-stub").exists()).toBe(true);
    expect(wrapper.find("file-tree-stub").exists()).toBe(false);
    wrapper.unmount();
  });

  test("页签使用 tablist 语义且只高亮当前项", () => {
    const wrapper = mountSidebar();
    expect(wrapper.find('[role="tablist"]').exists()).toBe(true);
    const tabs = wrapper.findAll('[role="tab"]');
    expect(tabs).toHaveLength(2);
    expect(tabs.filter((tab) => tab.attributes("aria-selected") === "true")).toHaveLength(1);
    wrapper.unmount();
  });
});
