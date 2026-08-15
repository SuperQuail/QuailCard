import { mount } from "@vue/test-utils";
import { afterEach, describe, expect, test } from "vitest";
import type { NoteSummary } from "../domain/types";
import FileTree from "./FileTree.vue";

/** 已挂载的组件，测试后统一卸载清理 Teleport 内容。 */
const mounted: ReturnType<typeof mount>[] = [];

afterEach(() => {
  for (const wrapper of mounted.splice(0)) {
    wrapper.unmount();
  }
});

/** 挂载文件树并记录以便清理。 */
function mountTree(props: {
  notes: NoteSummary[];
  folderNames: string[];
  activeNotePath: string | null;
  dueCount: number;
}): ReturnType<typeof mount> {
  const wrapper = mount(FileTree, { props });
  mounted.push(wrapper);
  return wrapper;
}

/** 构造测试用的笔记摘要。 */
function testNote(path: string): NoteSummary {
  return {
    path,
    title: path.split("/").pop()?.replace(/\.md$/, "") ?? path,
    tagsJson: "[]",
    cardCount: 0,
    dueCount: 0,
    mtime: 1,
  };
}

describe("FileTree 右键菜单", () => {
  test("右键文件夹行显示菜单", async () => {
    const wrapper = mountTree({
      notes: [testNote("英语/单词.md")],
      folderNames: ["英语"],
      activeNotePath: null,
      dueCount: 0,
    });
    const folderRow = wrapper.findAll("button").find((button) => button.text().includes("英语"));
    expect(folderRow).toBeDefined();
    await folderRow!.trigger("contextmenu", { clientX: 60, clientY: 60 });
    expect(document.body.textContent).toContain("新建笔记");
    expect(document.body.textContent).toContain("删除");
  });

  test("右键笔记行显示重命名与删除", async () => {
    const wrapper = mountTree({
      notes: [testNote("英语/单词.md")],
      folderNames: ["英语"],
      activeNotePath: null,
      dueCount: 0,
    });
    const noteRow = wrapper.findAll("button").find((button) => button.text().includes("单词"));
    expect(noteRow).toBeDefined();
    await noteRow!.trigger("contextmenu", { clientX: 60, clientY: 60 });
    expect(document.body.textContent).toContain("重命名");
  });

  test("点击空白处关闭菜单", async () => {
    const wrapper = mountTree({
      notes: [testNote("英语/单词.md")],
      folderNames: ["英语"],
      activeNotePath: null,
      dueCount: 0,
    });
    const folderRow = wrapper.findAll("button").find((button) => button.text().includes("英语"));
    await folderRow!.trigger("contextmenu", { clientX: 60, clientY: 60 });
    expect(document.body.textContent).toContain("新建笔记");
    const backdrop = document.querySelector(".z-70") as HTMLElement | null;
    expect(backdrop).not.toBeNull();
    backdrop?.dispatchEvent(new MouseEvent("mousedown", { bubbles: true }));
    await wrapper.vm.$nextTick();
    expect(document.body.textContent).not.toContain("新建笔记");
  });

  test("Shift 点击选择范围后按 Delete 批量删除", async () => {
    const wrapper = mountTree({
      notes: [testNote("英语/甲.md"), testNote("英语/乙.md")],
      folderNames: ["英语"],
      activeNotePath: null,
      dueCount: 0,
    });
    const firstRow = wrapper.findAll("button").find((button) => button.text().includes("甲"));
    const secondRow = wrapper.findAll("button").find((button) => button.text().includes("乙"));
    expect(firstRow).toBeDefined();
    expect(secondRow).toBeDefined();
    await firstRow!.trigger("click");
    await secondRow!.trigger("click", { shiftKey: true });
    const tree = wrapper.find("[tabindex='-1']");
    await tree.trigger("keydown", { key: "Delete" });
    const emitted = wrapper.emitted("delete-selection");
    expect(emitted).toBeDefined();
    const items = emitted![0][0] as Array<{ kind: string; path: string }>;
    expect(items.length).toBe(2);
    expect(items.map((item) => item.kind)).toEqual(["note", "note"]);
  });
});
