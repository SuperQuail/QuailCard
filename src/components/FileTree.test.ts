import { Brain, Search } from "@lucide/vue";
import { mount } from "@vue/test-utils";
import { nextTick } from "vue";
import { afterEach, describe, expect, test, vi } from "vitest";
import type { NoteSummary } from "../domain/types";
import FileTree from "./FileTree.vue";

/** 已挂载的组件，测试后统一卸载清理 Teleport 内容。 */
const mounted: ReturnType<typeof mount>[] = [];

afterEach(() => {
  for (const wrapper of mounted.splice(0)) {
    wrapper.unmount();
  }
  vi.useRealTimers();
});

/**
 * 直接派发指针事件：拖拽链路按事件目标判定落点，
 * @vue/test-utils 的 trigger 会往事件对象回写只读字段，用真实事件更贴近运行环境。
 */
async function pointer(element: EventTarget, type: string, init: MouseEventInit = {}): Promise<void> {
  element.dispatchEvent(new PointerEvent(type, { bubbles: true, cancelable: true, button: 0, ...init }));
  await nextTick();
}

/** 从源行按下并拖到目标元素，再松手：覆盖按下、越阈值、判定落点、落下四步。 */
async function dragOnto(from: Element, to: EventTarget): Promise<void> {
  await pointer(from, "pointerdown", { clientX: 10, clientY: 10 });
  await pointer(to, "pointermove", { clientX: 40, clientY: 40 });
  await pointer(window, "pointerup");
}

/** 按文本查找树行。 */
function rowByText(wrapper: ReturnType<typeof mount>, text: string) {
  const row = wrapper.findAll(".tree-row").find((candidate) => candidate.text().includes(text));
  if (!row) throw new Error(`未找到包含 ${text} 的树行`);
  return row;
}

/** 挂载文件树并记录以便清理；挂到 document 上，拖拽链路的事件才能冒泡到 window。 */
function mountTree(props: {
  notes: NoteSummary[];
  folderNames: string[];
  activeNotePath: string | null;
  dueCount: number;
}): ReturnType<typeof mount> {
  const wrapper = mount(FileTree, { props, attachTo: document.body });
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

/** 搜索仅筛选当前摘要，笔记打开与复习仍委派父组件。 */
describe("FileTree 工作区入口", () => {
  /** 搜索可以直接输入，不再弹出命令面板或误触发笔记跳转。 */
  test("标题数量下显示可输入的搜索框", async () => {
    const wrapper = mountTree({ notes: [testNote("笔记.md")], folderNames: [], activeNotePath: null, dueCount: 3 });
    expect(wrapper.get("header").text()).toContain("1 篇笔记");
    const search = wrapper.get('input[aria-label="搜索笔记"]');
    expect(search.attributes("type")).toBe("search");
    expect(wrapper.get(".tree-search").findComponent(Search).exists()).toBe(true);
    await search.setValue("笔记");
    expect(wrapper.emitted("open-palette")).toBeUndefined();
    expect(wrapper.emitted("select-note")).toBeUndefined();
    expect(wrapper.emitted("open-review")).toBeUndefined();
  });

  /** 零到期仍可进入复习工作区，数字与图标不改变事件契约。 */
  test.each([0, 12])("底部复习入口保留 Brain 与到期数 %i", async (dueCount) => {
    const wrapper = mountTree({ notes: [], folderNames: [], activeNotePath: null, dueCount });
    const footer = wrapper.get("footer");
    expect(footer.classes()).toContain("shrink-0");
    expect(footer.classes()).toContain("mt-auto");
    expect(footer.findComponent(Brain).exists()).toBe(true);
    expect(footer.text()).toContain("今日待复习");
    expect(footer.get(".font-semibold").text()).toBe(String(dueCount));
    await footer.get("button").trigger("click");
    expect(wrapper.emitted("open-review")).toEqual([[]]);
  });

  /** 标题入口不能依赖鼠标悬停，新建仍使用当前笔记的目录。 */
  test("新建按钮常显可聚焦并保留笔记和文件夹创建事件", async () => {
    const wrapper = mountTree({ notes: [testNote("英语/单词.md")], folderNames: ["英语"], activeNotePath: "英语/单词.md", dueCount: 0 });
    for (const button of wrapper.findAll("header button")) {
      expect(button.classes()).not.toContain("opacity-0");
      expect((button.element as HTMLButtonElement).tabIndex).toBe(0);
    }
    await wrapper.get('header button[title="新建笔记"]').trigger("click");
    await wrapper.get("input.tree-input").setValue("新笔记");
    await wrapper.get("input.tree-input").trigger("keyup.enter");
    expect(wrapper.emitted("note-created")).toEqual([["英语", "新笔记"]]);
    await wrapper.get('header button[title="新建文件夹"]').trigger("click");
    await wrapper.get("input.tree-input").setValue("新目录");
    await wrapper.get("input.tree-input").trigger("keyup.enter");
    expect(wrapper.emitted("folder-created")).toEqual([["新目录"]]);
  });

  /** 增大行距不改变展开、选中和笔记导航的路径语义。 */
  test("文件夹展开收起与笔记导航继续发送原路径", async () => {
    const wrapper = mountTree({ notes: [testNote("英语/单词.md")], folderNames: ["英语"], activeNotePath: null, dueCount: 0 });
    await wrapper.get(".tree-row").trigger("click");
    expect(wrapper.findAll(".tree-row")).toHaveLength(1);
    expect(wrapper.emitted("select-note")).toBeUndefined();
    await wrapper.get(".tree-row").trigger("click");
    await wrapper.findAll(".tree-row")[1].trigger("click");
    expect(wrapper.emitted("select-note")).toEqual([["英语/单词.md"]]);
    expect(wrapper.findAll(".tree-row")[1].classes()).toContain("is-selected");
  });
});

describe("FileTree 即时搜索", () => {
  /** 搜索能找到折叠目录下的笔记，大小写与首尾空格不影响匹配。 */
  test("按标题和路径匹配，清空恢复目录折叠状态", async () => {
    const wrapper = mountTree({ notes: [testNote("英语/Godot.md"), testNote("英语/单词.md"), testNote("课程/线性表.md")], folderNames: ["英语", "课程"], activeNotePath: "英语/Godot.md", dueCount: 0 });
    await wrapper.get(".tree-row").trigger("click");
    const original = wrapper.findAll(".tree-row").map((row) => row.text());
    const input = wrapper.get('input[aria-label="搜索笔记"]');
    await input.setValue("  gOdOt  ");
    expect(wrapper.findAll(".tree-row")).toHaveLength(1);
    expect(wrapper.get(".tree-row").attributes("title")).toBe("英语/Godot.md");
    expect(wrapper.get(".tree-row").classes()).toContain("bg-bg-active");
    expect(wrapper.find(".tree-guide").exists()).toBe(false);
    await input.setValue("英语/");
    expect(wrapper.findAll(".tree-row")).toHaveLength(2);
    await wrapper.get('[aria-label="清空搜索"]').trigger("click");
    expect(wrapper.findAll(".tree-row").map((row) => row.text())).toEqual(original);
    expect(wrapper.find(".tree-guide").exists()).toBe(false);
  });

  /** 回车打开匹配项，但输入法确认候选必须只影响文本输入。 */
  test("回车打开匹配笔记，输入法候选不跳转，Esc清空", async () => {
    const wrapper = mountTree({ notes: [testNote("课程/线性表.md")], folderNames: ["课程"], activeNotePath: null, dueCount: 0 });
    const input = wrapper.get('input[aria-label="搜索笔记"]');
    await input.setValue("线性");
    await input.trigger("keydown", { key: "Enter", isComposing: true });
    expect(wrapper.emitted("select-note")).toBeUndefined();
    await input.trigger("keydown", { key: "Enter" });
    expect(wrapper.emitted("select-note")).toEqual([["课程/线性表.md"]]);
    await input.trigger("keydown", { key: "Escape" });
    expect((input.element as HTMLInputElement).value).toBe("");
    expect(wrapper.findAll(".tree-row")).toHaveLength(2);
  });

  /** 筛选不保留不可见的多选，零结果不能跳转或误删旧目标。 */
  test("无结果有提示且清理旧选中项", async () => {
    const wrapper = mountTree({ notes: [testNote("甲.md"), testNote("乙.md")], folderNames: [], activeNotePath: "甲.md", dueCount: 0 });
    await wrapper.get(".tree-row").trigger("click");
    const input = wrapper.get('input[aria-label="搜索笔记"]');
    await input.setValue("不存在");
    expect(wrapper.get('[role="status"]').text()).toBe("没有匹配的笔记");
    expect(wrapper.findAll(".tree-row")).toHaveLength(0);
    await input.trigger("keydown", { key: "Enter" });
    expect(wrapper.emitted("select-note")).toHaveLength(1);
    await input.setValue("");
    await wrapper.get('[tabindex="-1"]').trigger("keydown", { key: "Delete" });
    expect(wrapper.emitted("delete-selection")).toBeUndefined();
    expect(wrapper.get(".tree-row.is-active").text()).toBe("甲");
  });

  /** 新建时退出筛选，让新建输入框不会被结果列表隐藏。 */
  test("搜索状态点击新建仍展示原目录输入框", async () => {
    const wrapper = mountTree({ notes: [testNote("课程/线性表.md")], folderNames: ["课程"], activeNotePath: "课程/线性表.md", dueCount: 0 });
    await wrapper.get('input[aria-label="搜索笔记"]').setValue("线性");
    await wrapper.get('header button[title="新建笔记"]').trigger("click");
    expect((wrapper.get('input[aria-label="搜索笔记"]').element as HTMLInputElement).value).toBe("");
    expect(wrapper.find("input.tree-input").exists()).toBe(true);
  });
});

describe("FileTree 右键菜单", () => {
  test("目录改名保持后代展开和选择，名称无需窗口重绘即可更新", async () => {
    const wrapper = mountTree({ notes: [testNote("旧/子/笔记.md")], folderNames: ["旧", "旧/子"], activeNotePath: "旧/子/笔记.md", dueCount: 0 });
    const note = wrapper.findAll(".tree-row").find((row) => row.text().includes("笔记"))!;
    await note.trigger("click");
    await wrapper.setProps({ notes: [testNote("新/子/笔记.md")], folderNames: ["新", "新/子"], activeNotePath: "新/子/笔记.md", pathChange: { oldPath: "旧", newPath: "新" } });
    expect(wrapper.text()).toContain("笔记");
    expect(wrapper.find(".tree-row.is-selected").text()).toContain("笔记");
    expect(wrapper.findAll(".tree-row").map((row) => row.text())).toEqual(["新", "子", "笔记"]);
  });

  test("改名失败保留输入，回车与失焦不会重复提交", async () => {
    const wrapper = mountTree({ notes: [testNote("笔记.md")], folderNames: [], activeNotePath: "笔记.md", dueCount: 0 });
    await wrapper.find(".tree-row").trigger("contextmenu");
    const rename = [...document.body.querySelectorAll("button")].find((button) => button.textContent?.trim() === "重命名")!;
    rename.click();
    await wrapper.vm.$nextTick();
    const input = wrapper.find("input.tree-input");
    await input.setValue("新名称");
    await input.trigger("keyup.enter");
    await input.trigger("blur");
    const events = wrapper.emitted("rename-note")!;
    expect(events).toHaveLength(1);
    (events[0][2] as (error?: string) => void)("同名笔记已存在");
    await Promise.resolve(); await wrapper.vm.$nextTick();
    expect((wrapper.find("input.tree-input").element as HTMLInputElement).value).toBe("新名称");
    expect(wrapper.text()).toContain("同名笔记已存在");
  });

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

describe("FileTree 拖拽移动", () => {
  /** 笔记拖到文件夹行：整组移动，落点就是目标文件夹。 */
  test("笔记拖入文件夹行提交移动计划", async () => {
    const wrapper = mountTree({ notes: [testNote("单词.md")], folderNames: ["英语"], activeNotePath: null, dueCount: 0 });
    const noteRow = rowByText(wrapper, "单词");
    const folderRow = rowByText(wrapper, "英语");
    await pointer(noteRow.element, "pointerdown", { clientX: 10, clientY: 10 });
    await pointer(folderRow.element, "pointermove", { clientX: 40, clientY: 40 });
    expect(folderRow.classes()).toContain("is-drop-target");
    expect(noteRow.classes()).toContain("is-dragging");
    expect(wrapper.text()).toContain("松开后移动 1 项到「英语」");
    await pointer(window, "pointerup");
    expect(wrapper.emitted("move-entries")).toEqual([[[{ kind: "note", from: "单词.md", to: "英语/单词.md" }]]]);
    expect(folderRow.classes()).not.toContain("is-drop-target");
    expect(noteRow.classes()).not.toContain("is-dragging");
  });

  /** 拖到笔记行等于放进该笔记所在目录；拖到树内空白等于回到根目录。 */
  test("笔记行与空白处同样是落点", async () => {
    const wrapper = mountTree({ notes: [testNote("英语/单词.md"), testNote("数学/线性代数.md")], folderNames: ["英语", "数学"], activeNotePath: null, dueCount: 0 });
    await dragOnto(rowByText(wrapper, "线性代数").element, rowByText(wrapper, "单词").element);
    expect(wrapper.emitted("move-entries")![0][0]).toEqual([{ kind: "note", from: "数学/线性代数.md", to: "英语/线性代数.md" }]);
    await dragOnto(rowByText(wrapper, "单词").element, wrapper.get("[tabindex='-1']").element);
    expect(wrapper.emitted("move-entries")![1][0]).toEqual([{ kind: "note", from: "英语/单词.md", to: "单词.md" }]);
  });

  /** 落进折叠中的目录后立即展开，让移动结果可见。 */
  test("落进折叠目录后展开它", async () => {
    const wrapper = mountTree({ notes: [testNote("随记.md"), testNote("课程/数据结构/线性表.md")], folderNames: ["课程", "课程/数据结构"], activeNotePath: null, dueCount: 0 });
    await rowByText(wrapper, "课程").trigger("click");
    expect(wrapper.text()).not.toContain("数据结构");
    await dragOnto(rowByText(wrapper, "随记").element, rowByText(wrapper, "课程").element);
    expect(wrapper.emitted("move-entries")![0][0]).toEqual([{ kind: "note", from: "随记.md", to: "课程/随记.md" }]);
    expect(wrapper.text()).toContain("数据结构");
  });

  /** 文件夹不能拖进自身或自身子目录，此时不显示落点也不提交移动。 */
  test("文件夹拖进自身子树不产生移动", async () => {
    const wrapper = mountTree({ notes: [testNote("英语/单词.md")], folderNames: ["英语"], activeNotePath: null, dueCount: 0 });
    const folderRow = rowByText(wrapper, "英语");
    await pointer(folderRow.element, "pointerdown", { clientX: 10, clientY: 10 });
    await pointer(folderRow.element, "pointermove", { clientX: 40, clientY: 40 });
    expect(folderRow.classes()).not.toContain("is-drop-target");
    await pointer(window, "pointerup");
    expect(wrapper.emitted("move-entries")).toBeUndefined();
  });

  /** 多选拖拽整组移动，顺序按树内可见顺序。 */
  test("多选行整组移动", async () => {
    const wrapper = mountTree({ notes: [testNote("甲.md"), testNote("乙.md")], folderNames: ["课程"], activeNotePath: null, dueCount: 0 });
    await rowByText(wrapper, "甲").trigger("click");
    await rowByText(wrapper, "乙").trigger("click", { ctrlKey: true });
    await dragOnto(rowByText(wrapper, "甲").element, rowByText(wrapper, "课程").element);
    expect(wrapper.emitted("move-entries")![0][0]).toEqual([
      { kind: "note", from: "甲.md", to: "课程/甲.md" },
      { kind: "note", from: "乙.md", to: "课程/乙.md" },
    ]);
  });

  /** 拖拽结束后紧跟的 click 只能用于收尾，不能顺手打开笔记。 */
  test("拖拽收尾的点击不打开笔记", async () => {
    const wrapper = mountTree({ notes: [testNote("单词.md")], folderNames: ["英语"], activeNotePath: null, dueCount: 0 });
    const noteRow = rowByText(wrapper, "单词");
    await dragOnto(noteRow.element, rowByText(wrapper, "英语").element);
    await noteRow.trigger("click");
    expect(wrapper.emitted("select-note")).toBeUndefined();
    // 下一次真实点击恢复原行为。
    await noteRow.trigger("click");
    expect(wrapper.emitted("select-note")).toEqual([["单词.md"]]);
  });

  /** 悬停折叠文件夹会自动展开，拖进看不见的目录不必先手动展开。 */
  test("悬停折叠文件夹延时展开", async () => {
    const wrapper = mountTree({ notes: [testNote("随记.md"), testNote("课程/数据结构/线性表.md")], folderNames: ["课程", "课程/数据结构"], activeNotePath: null, dueCount: 0 });
    await rowByText(wrapper, "课程").trigger("click");
    // 折叠后只留下文件夹行（含计数徽章）与根级笔记。
    expect(wrapper.findAll(".tree-row")).toHaveLength(2);
    expect(wrapper.text()).toContain("随记");
    expect(wrapper.text()).not.toContain("数据结构");
    vi.useFakeTimers();
    await pointer(rowByText(wrapper, "随记").element, "pointerdown", { clientX: 10, clientY: 10 });
    await pointer(rowByText(wrapper, "课程").element, "pointermove", { clientX: 40, clientY: 40 });
    vi.advanceTimersByTime(700);
    await nextTick();
    // 展开后子文件夹与更深一层的笔记一起出现（子文件夹原本就是展开状态）。
    expect(wrapper.findAll(".tree-row").map((row) => row.text().replace(/\d+$/, ""))).toEqual(["课程", "数据结构", "线性表", "随记"]);
    await pointer(window, "pointerup");
  });

  /** Escape 取消拖拽，不执行任何移动也不留下高亮。 */
  test("Escape 取消拖拽", async () => {
    const wrapper = mountTree({ notes: [testNote("单词.md")], folderNames: ["英语"], activeNotePath: null, dueCount: 0 });
    const noteRow = rowByText(wrapper, "单词");
    const folderRow = rowByText(wrapper, "英语");
    await pointer(noteRow.element, "pointerdown", { clientX: 10, clientY: 10 });
    await pointer(folderRow.element, "pointermove", { clientX: 40, clientY: 40 });
    window.dispatchEvent(new KeyboardEvent("keydown", { key: "Escape", bubbles: true }));
    await nextTick();
    expect(folderRow.classes()).not.toContain("is-drop-target");
    await pointer(window, "pointerup");
    expect(wrapper.emitted("move-entries")).toBeUndefined();
  });

  /** 拖到树外（编辑器等区域）不产生落点，松手也不执行移动。 */
  test("指针离开树后不再显示落点", async () => {
    const wrapper = mountTree({ notes: [testNote("单词.md")], folderNames: ["英语"], activeNotePath: null, dueCount: 0 });
    const folderRow = rowByText(wrapper, "英语");
    await pointer(rowByText(wrapper, "单词").element, "pointerdown", { clientX: 10, clientY: 10 });
    await pointer(folderRow.element, "pointermove", { clientX: 40, clientY: 40 });
    expect(folderRow.classes()).toContain("is-drop-target");
    const outside = document.createElement("div");
    document.body.appendChild(outside);
    await pointer(outside, "pointermove", { clientX: 400, clientY: 40 });
    expect(folderRow.classes()).not.toContain("is-drop-target");
    await pointer(window, "pointerup");
    expect(wrapper.emitted("move-entries")).toBeUndefined();
    outside.remove();
  });

  /** 不足阈值的移动仍是点击：不进入拖拽，也不吞掉随后的 click。 */
  test("轻微移动不进入拖拽", async () => {
    const wrapper = mountTree({ notes: [testNote("单词.md")], folderNames: ["英语"], activeNotePath: null, dueCount: 0 });
    const noteRow = rowByText(wrapper, "单词");
    await pointer(noteRow.element, "pointerdown", { clientX: 10, clientY: 10 });
    await pointer(noteRow.element, "pointermove", { clientX: 11, clientY: 11 });
    expect(noteRow.classes()).not.toContain("is-dragging");
    await pointer(window, "pointerup");
    await noteRow.trigger("click");
    expect(wrapper.emitted("select-note")).toEqual([["单词.md"]]);
  });
});
