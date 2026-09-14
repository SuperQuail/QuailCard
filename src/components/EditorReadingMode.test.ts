import { mount } from "@vue/test-utils";
import { EditorView } from "@codemirror/view";
import { undo, undoDepth } from "@codemirror/commands";
import { afterEach, expect, test } from "vitest";
import EditorPane from "./EditorPane.vue";

const mounted: ReturnType<typeof mount>[] = [];
/** 卸载真实编辑器，释放文档监听及异步测绘。 */
afterEach(() => { mounted.splice(0).forEach((wrapper) => wrapper.unmount()); });
/** 保留真实编辑器与工具栏，验证阅读切换的完整状态通路。 */
function editor(content = "# 标题\n\n测试正文") {
  const wrapper = mount(EditorPane, { props: {
    notePath: "课程/笔记.md", content, dark: false,
    panelOpen: false,
  } });
  mounted.push(wrapper);
  const view = EditorView.findFromDOM(wrapper.get(".cm-editor").element as HTMLElement)!;
  return { wrapper, view };
}

/** 阅读不是另建一个文档，切换不保存、不清空历史或原选区。 */
test("阅读禁止输入事务，返回编辑恢复选区与撤销历史", async () => {
  const { wrapper, view } = editor();
  view.dispatch({ changes: { from: view.state.doc.length, insert: "补充" }, selection: { anchor: 2 } });
  const before = view.state.doc.toString();
  const history = undoDepth(view.state);
  const saves = wrapper.emitted("save-content")!.length;
  await wrapper.get('[aria-label="切换到阅读模式"]').trigger("click");
  expect(view.state.readOnly).toBe(true);
  expect(view.contentDOM.getAttribute("contenteditable")).toBe("false");
  expect(view.contentDOM.getAttribute("role")).toBe("document");
  expect(wrapper.find(".cm-activeLine").exists()).toBe(false);
  view.dispatch({ changes: { from: 0, insert: "不可写" } });
  expect(view.state.doc.toString()).toBe(before);
  expect(undo(view)).toBe(false);
  view.dispatch({ selection: { anchor: 0, head: 1 } });
  expect(wrapper.vm.getSelection()).toBeNull();
  await wrapper.get('[aria-label="切换到编辑模式"]').trigger("click");
  expect(EditorView.findFromDOM(wrapper.get(".cm-editor").element as HTMLElement)).toBe(view);
  expect(view.state.readOnly).toBe(false);
  expect(view.state.selection.main.anchor).toBe(2);
  expect(undoDepth(view.state)).toBe(history);
  expect(wrapper.emitted("save-content")).toHaveLength(saves);
  expect(undo(view)).toBe(true);
  expect(view.state.doc.toString()).toBe("# 标题\n\n测试正文");
});

/** 删除保存提示不影响阅读保护，退出阅读仍不能绕过文件操作锁。 */
test("只读文件锁独立于阅读开关", async () => {
  const { wrapper, view } = editor();
  await wrapper.get('[aria-label="切换到阅读模式"]').trigger("click");
  expect(wrapper.find('[role="status"]').exists()).toBe(false);
  await wrapper.setProps({ readOnly: true });
  await wrapper.get('[aria-label="切换到编辑模式"]').trigger("click");
  expect(view.state.readOnly).toBe(true);
  expect(view.contentDOM.getAttribute("contenteditable")).toBe("false");
  await wrapper.setProps({ readOnly: false });
  expect(view.state.readOnly).toBe(false);
  expect(view.contentDOM.getAttribute("contenteditable")).toBe("true");
});

/** 已确认外部内容可同步到阅读视图，旧光标随替换映射且不重复触发保存。 */
test("阅读时同步外部内容并安全切换笔记", async () => {
  const { wrapper, view } = editor();
  view.dispatch({ selection: { anchor: view.state.doc.length } });
  await wrapper.get('[aria-label="切换到阅读模式"]').trigger("click");
  await wrapper.setProps({ content: "短文", dark: true, panelOpen: true });
  expect(view.state.doc.toString()).toBe("短文");
  expect(view.state.readOnly).toBe(true);
  expect(wrapper.emitted("save-content")).toBeUndefined();
  await wrapper.get('[aria-label="切换到编辑模式"]').trigger("click");
  expect(view.state.selection.main.anchor).toBeLessThanOrEqual(2);
  await wrapper.get('[aria-label="切换到阅读模式"]').trigger("click");
  await wrapper.setProps({ notePath: "其他.md", content: "新的笔记" });
  const other = EditorView.findFromDOM(wrapper.get(".cm-editor").element as HTMLElement)!;
  expect(other).not.toBe(view);
  expect(other.state.readOnly).toBe(true);
  expect(other.state.doc.toString()).toBe("新的笔记");
  await wrapper.get('[aria-label="切换到编辑模式"]').trigger("click");
  expect(other.state.selection.main.anchor).toBe(0);
  expect(wrapper.emitted("save-content")).toBeUndefined();
});

/** 每篇笔记恢复自己的阅读开关及退出阅读原选区，不借用其他标签的光标。 */
test("标签各自保留阅读状态和返回编辑原选区", async () => {
  const { wrapper, view } = editor();
  const content = view.state.doc.toString();
  view.dispatch({ selection: { anchor: 5, head: 2 } });
  await wrapper.get('[aria-label="切换到阅读模式"]').trigger("click");
  view.dispatch({ selection: { anchor: 0, head: 1 } });
  await wrapper.setProps({ notePath: "另一篇.md", content: "其他正文" });
  const second = EditorView.findFromDOM(wrapper.get(".cm-editor").element as HTMLElement)!;
  expect(second.state.readOnly).toBe(true);
  await wrapper.get('[aria-label="切换到编辑模式"]').trigger("click");
  expect(second.state.selection.main.anchor).toBe(0);
  second.dispatch({ selection: { anchor: 3 } });
  await wrapper.setProps({ notePath: "课程/笔记.md", content, dark: true });
  const restored = EditorView.findFromDOM(wrapper.get(".cm-editor").element as HTMLElement)!;
  expect(restored.state.readOnly).toBe(true);
  expect(restored.state.facet(EditorView.darkTheme)).toBe(true);
  expect(restored.state.selection.main.anchor).toBe(0);
  expect(restored.state.selection.main.head).toBe(1);
  await wrapper.get('[aria-label="切换到编辑模式"]').trigger("click");
  expect(restored.state.selection.main.anchor).toBe(5);
  expect(restored.state.selection.main.head).toBe(2);
  await wrapper.setProps({ notePath: "另一篇.md", content: "其他正文" });
  const other = EditorView.findFromDOM(wrapper.get(".cm-editor").element as HTMLElement)!;
  expect(other.state.readOnly).toBe(false);
  expect(other.state.selection.main.anchor).toBe(3);
  expect(wrapper.emitted("save-content")).toBeUndefined();
});

/** 非活跃正文被外部改短时只继承该笔记阅读开关，旧原选区不可越界。 */
test("外部替换缓存正文仍保留阅读状态并解除过期选区", async () => {
  const { wrapper, view } = editor();
  view.dispatch({ selection: { anchor: view.state.doc.length } });
  await wrapper.get('[aria-label="切换到阅读模式"]').trigger("click");
  await wrapper.setProps({ notePath: "另一篇.md", content: "其他" });
  await wrapper.get('[aria-label="切换到编辑模式"]').trigger("click");
  await wrapper.setProps({ notePath: "课程/笔记.md", content: "短", readOnly: true });
  const restored = EditorView.findFromDOM(wrapper.get(".cm-editor").element as HTMLElement)!;
  expect(restored.contentDOM.getAttribute("role")).toBe("document");
  expect(restored.state.doc.toString()).toBe("短");
  await wrapper.get('[aria-label="切换到编辑模式"]').trigger("click");
  expect(restored.state.selection.main.anchor).toBeLessThanOrEqual(1);
  expect(restored.state.readOnly).toBe(true);
  await wrapper.setProps({ readOnly: false });
  expect(restored.state.readOnly).toBe(false);
  expect(wrapper.emitted("save-content")).toBeUndefined();
});
