import { flushPromises, mount } from "@vue/test-utils";
import { undo, undoDepth, redo } from "@codemirror/commands";
import { captureCardSource } from "../domain/cardSource";
import type { NoteCard } from "../domain/types";
import { importImageAttachment } from "../services/attachmentService";
import { EditorView } from "@codemirror/view";
import { afterEach, expect, test, vi } from "vitest";
import EditorPane from "./EditorPane.vue";

vi.mock("../services/attachmentService", () => ({
  importImageAttachment: vi.fn(), resolveAttachmentDataUrl: vi.fn().mockResolvedValue("data:image/png;base64,cG5n"),
}));
const mounted: ReturnType<typeof mount>[] = [];
/** 清理编辑器实例与全局选区监听，避免后续测试收到残留事件。 */
afterEach(() => { mounted.splice(0).forEach((wrapper) => wrapper.unmount()); });
/** 使用真实编辑器，确保布局变化不会重建文档或丢失光标。 */
function editor() {
  const wrapper = mount(EditorPane, { attachTo: document.body, props: {
    notePath: "课程/笔记.md", content: "# 标题\n\n测试正文", dark: false,
    panelOpen: false,
  } });
  mounted.push(wrapper);
  return wrapper;
}

/** 工具栏仅转发操作，移除保存提示不影响侧栏开关。 */
test("转发侧栏开关且不显示保存入口", async () => {
  const wrapper = editor();
  await wrapper.get('[aria-label="打开右侧栏"]').trigger("click");
  expect(wrapper.emitted("toggle-panel")).toEqual([[]]);
  expect(wrapper.find('[role="status"]').exists()).toBe(false);
  expect(wrapper.get("header").text()).toBe("");
});

/** 侧栏与主题更新不应更换编辑器实例或写回未改变的正文。 */
test("布局与主题更新保留编辑器和选区", async () => {
  const wrapper = editor();
  const dom = wrapper.get(".cm-editor").element as HTMLElement;
  const view = EditorView.findFromDOM(dom)!;
  view.dispatch({ selection: { anchor: 2 } });
  await wrapper.setProps({ panelOpen: true, dark: true });
  expect(wrapper.get(".cm-editor").element).toBe(dom);
  expect(view.state.selection.main.anchor).toBe(2);
  expect(view.state.doc.toString()).toBe("# 标题\n\n测试正文");
  expect(wrapper.emitted("save-content")).toBeUndefined();
  expect(wrapper.get("header").element.closest(".cm-scroller")).toBeNull();
  expect(wrapper.findAll(".cm-scroller")).toHaveLength(1);
  expect(wrapper.classes()).toContain("relative");
  expect(wrapper.get("header").classes()).toContain("absolute");
});

/** 每次从当前 DOM 获取真实实例，避免测试误操作已销毁的视图。 */
function current(wrapper: ReturnType<typeof editor>): EditorView {
  return EditorView.findFromDOM(wrapper.get(".cm-editor").element as HTMLElement)!;
}

/** 撤销与反向选区属于笔记；缓存绝不能为每个标签保留一个活编辑器。 */
test("跨笔记恢复撤销重做、选区与滚动且保存路径隔离", async () => {
  const wrapper = editor();
  const first = current(wrapper);
  const original = first.state.doc.toString();
  const draft = original + "补充";
  first.dispatch({ changes: { from: original.length, insert: "补充" }, selection: { anchor: 5, head: 2 } });
  await wrapper.setProps({ content: draft });
  first.scrollDOM.scrollTop = 91;
  first.scrollDOM.scrollLeft = 17;
  await wrapper.setProps({ notePath: "另一篇.md", content: "另一篇" });
  expect(first.dom.isConnected).toBe(false);
  const second = current(wrapper);
  expect(undoDepth(second.state)).toBe(0);
  second.dispatch({ changes: { from: 3, insert: "新字" } });
  await wrapper.setProps({ notePath: "课程/笔记.md", content: draft });
  const restored = current(wrapper);
  expect(restored).not.toBe(first);
  expect(second.dom.isConnected).toBe(false);
  expect(wrapper.findAll(".cm-editor")).toHaveLength(1);
  expect(restored.state.selection.main.anchor).toBe(5);
  expect(restored.state.selection.main.head).toBe(2);
  expect(restored.scrollDOM.scrollTop).toBe(91);
  expect(restored.scrollDOM.scrollLeft).toBe(17);
  expect(undo(restored)).toBe(true);
  expect(restored.state.doc.toString()).toBe(original);
  expect(redo(restored)).toBe(true);
  expect(restored.state.doc.toString()).toBe(draft);
  await wrapper.setProps({ notePath: "另一篇.md", content: "另一篇新字" });
  expect(undo(current(wrapper))).toBe(true);
  expect(current(wrapper).state.doc.toString()).toBe("另一篇");
  expect(wrapper.emitted("save-content")).toEqual([
    ["课程/笔记.md", draft], ["另一篇.md", "另一篇新字"],
    ["课程/笔记.md", original], ["课程/笔记.md", draft], ["另一篇.md", "另一篇"],
  ]);
});

/** 活跃同步不产生保存或可撤销的外部更新；非活跃旧草稿不覆盖重新读取的文件。 */
test("已确认外部内容优先于缓存且不重复保存", async () => {
  const wrapper = editor();
  await wrapper.setProps({ content: "外部正文" });
  expect(current(wrapper).state.doc.toString()).toBe("外部正文");
  expect(undoDepth(current(wrapper).state)).toBe(0);
  expect(wrapper.emitted("save-content")).toBeUndefined();
  current(wrapper).dispatch({ changes: { from: 4, insert: "旧草稿" } });
  await wrapper.setProps({ notePath: "另一篇.md", content: "其他" });
  await wrapper.setProps({ notePath: "课程/笔记.md", content: "磁盘更新" });
  expect(current(wrapper).state.doc.toString()).toBe("磁盘更新");
  expect(undoDepth(current(wrapper).state)).toBe(0);
  expect(undo(current(wrapper))).toBe(false);
  expect(wrapper.emitted("save-content")).toEqual([["课程/笔记.md", "外部正文旧草稿"]]);
});

/** 恢复笔记时使用最新主题、文件锁和卡片列表，而不是缓存中的旧 compartment。 */
test("缓存恢复重新配置主题只读与卡片装饰", async () => {
  const wrapper = editor();
  const content = current(wrapper).state.doc.toString();
  const card = { id: "new-card", source: captureCardSource(content, 6, 8) } as NoteCard;
  await wrapper.setProps({ notePath: "另一篇.md", content: "其他", dark: true, readOnly: true });
  await wrapper.setProps({ notePath: "课程/笔记.md", content, cards: [card] });
  const restored = current(wrapper);
  expect(restored.state.facet(EditorView.darkTheme)).toBe(true);
  expect(restored.state.readOnly).toBe(true);
  expect(restored.contentDOM.getAttribute("contenteditable")).toBe("false");
  expect(wrapper.findAll(".qc-anchor-chip")).toHaveLength(1);
  await wrapper.get(".qc-anchor-chip").trigger("click");
  expect(wrapper.emitted("card-click")).toEqual([["new-card"]]);
  await wrapper.setProps({ notePath: "另一篇.md", content: "其他", dark: false, readOnly: false, cards: [] });
  await wrapper.setProps({ notePath: "课程/笔记.md", content });
  expect(current(wrapper).state.facet(EditorView.darkTheme)).toBe(false);
  expect(current(wrapper).state.readOnly).toBe(false);
  expect(wrapper.find(".qc-anchor-chip").exists()).toBe(false);
  expect(wrapper.emitted("save-content")).toBeUndefined();
});

/** 活跃标签关闭与普通关闭都要淘汰历史，重新打开同名文件视为新标签。 */
test.each([false, true])("关闭标签不保留被移除笔记的缓存（活跃=%s）", async (active) => {
  const wrapper = editor();
  const tabs = [{ path: "课程/笔记.md", title: "笔记" }, { path: "另一篇.md", title: "另一篇" }];
  await wrapper.setProps({ tabs });
  const draft = current(wrapper).state.doc.toString() + "补充";
  current(wrapper).dispatch({ changes: { from: current(wrapper).state.doc.length, insert: "补充" } });
  await wrapper.setProps({ notePath: "另一篇.md", content: "其他", tabs: active ? tabs.slice(1) : tabs });
  if (!active) await wrapper.setProps({ tabs: tabs.slice(1) });
  await wrapper.setProps({ tabs, notePath: "课程/笔记.md", content: draft });
  expect(undoDepth(current(wrapper).state)).toBe(0);
  expect(wrapper.findAll(".cm-editor")).toHaveLength(1);
});

/** 旧 View 销毁后即使又回到原路径，迟到图片也不能插入或继续写第二张附件。 */
test("切走再切回后旧图片请求不会污染恢复的笔记", async () => {
  const wrapper = editor();
  const first = current(wrapper);
  const content = first.state.doc.toString();
  let finish!: (value: Awaited<ReturnType<typeof importImageAttachment>>) => void;
  vi.mocked(importImageAttachment).mockImplementationOnce(() => new Promise((resolve) => { finish = resolve; }));
  const event = new Event("paste", { bubbles: true, cancelable: true });
  Object.defineProperty(event, "clipboardData", { value: { files: [
    new File(["png"], "one.png", { type: "image/png" }), new File(["png"], "two.png", { type: "image/png" }),
  ] } });
  first.contentDOM.dispatchEvent(event);
  expect(importImageAttachment).toHaveBeenCalledTimes(1);
  await wrapper.setProps({ notePath: "另一篇.md", content: "其他" });
  await wrapper.setProps({ notePath: "课程/笔记.md", content });
  expect(wrapper.find(".qc-upload-marker").exists()).toBe(false);
  finish({ markdownPath: "attachments/one.png" });
  await flushPromises();
  expect(importImageAttachment).toHaveBeenCalledTimes(1);
  expect(current(wrapper).state.doc.toString()).toBe(content);
  expect(wrapper.emitted("save-content")).toBeUndefined();
  vi.mocked(importImageAttachment).mockResolvedValueOnce({ markdownPath: "attachments/new.png" });
  const freshPaste = new Event("paste", { bubbles: true, cancelable: true });
  Object.defineProperty(freshPaste, "clipboardData", { value: { files: [new File(["png"], "new.png", { type: "image/png" })] } });
  current(wrapper).contentDOM.dispatchEvent(freshPaste);
  await flushPromises();
  expect(importImageAttachment).toHaveBeenCalledTimes(2);
  expect(current(wrapper).state.doc.toString()).toContain("![new](attachments/new.png)");
  expect(wrapper.emitted("save-content")).toHaveLength(1);
  expect(wrapper.emitted("save-content")![0][0]).toBe("课程/笔记.md");
});

