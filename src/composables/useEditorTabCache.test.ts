import { afterEach, expect, test } from "vitest";
import { history, undo, undoDepth } from "@codemirror/commands";
import { EditorView } from "@codemirror/view";
import { useEditorTabCache } from "./useEditorTabCache";

const views: EditorView[] = [];
const reading = { reading: false, editSelection: null };
/** 不让缓存测试的编辑器监听残留在后续测试中。 */
afterEach(() => { views.splice(0).forEach((view) => view.destroy()); });

/** 用真实 CM 状态保存历史，测试不使用伪造的 state 或 DOM。 */
function editor(): EditorView {
  const view = new EditorView({ doc: "正文", extensions: [history()], parent: document.body });
  views.push(view);
  view.dispatch({ changes: { from: 2, insert: "草稿" }, selection: { anchor: 1, head: 3 } });
  return view;
}

/** State 和阅读选区是不可变快照，激活取出后不得仍计入非活跃缓存。 */
test("取出原始state可恢复历史选区且不保留重复活跃项", () => {
  const cache = useEditorTabCache();
  const view = editor();
  const state = view.state;
  const snapshot = { reading: true, editSelection: state.selection };
  view.scrollDOM.scrollTop = 120;
  view.scrollDOM.scrollLeft = 16;
  cache.save("a.md", view, snapshot);
  const saved = cache.take("a.md", "正文草稿")!;
  expect(saved.state).toBe(state);
  expect(saved.reading).toBe(snapshot);
  expect(saved.scroll?.effect).toBeDefined();
  const restored = new EditorView({ state: saved.state, parent: document.body });
  views.push(restored);
  cache.restoreScroll(restored, saved);
  expect(restored.scrollDOM.scrollTop).toBe(120);
  expect(restored.scrollDOM.scrollLeft).toBe(16);
  expect(restored.state.selection.main.anchor).toBe(1);
  expect(restored.state.selection.main.head).toBe(3);
  expect(undoDepth(restored.state)).toBe(1);
  expect(undo(restored)).toBe(true);
  expect(restored.state.doc.toString()).toBe("正文");
  expect(cache.take("a.md", "正文草稿")).toBeUndefined();
});

/** 外部确认内容不匹配时不能复用旧草稿历史，但阅读偏好仍属于该笔记。 */
test("外部正文变化丢弃state滚动及过期原选区", () => {
  const cache = useEditorTabCache();
  const view = editor();
  cache.save("a.md", view, { reading: true, editSelection: view.state.selection });
  expect(cache.take("a.md", "外部")).toEqual({ reading: { reading: true, editSelection: null } });
});

/** 非活跃缓存有界，访问并离开会刷新顺序而不淘汰较新的笔记。 */
test("LRU只保留限定数量的非活跃状态", () => {
  const cache = useEditorTabCache(2);
  const view = editor();
  cache.save("a.md", view, reading);
  cache.save("b.md", view, reading);
  expect(cache.take("a.md", "正文草稿")).toBeDefined();
  cache.save("a.md", view, reading);
  cache.save("c.md", view, reading);
  expect(cache.take("b.md", "正文草稿")).toBeUndefined();
  expect(cache.take("a.md", "正文草稿")).toBeDefined();
  expect(cache.take("c.md", "正文草稿")).toBeDefined();
});

/** 默认20项限制长会话占用，也允许调用方以零容量关闭缓存。 */
test("默认容量20且零容量不缓存", () => {
  const view = editor();
  const cache = useEditorTabCache();
  for (let i = 0; i <= 20; i++) cache.save(String(i), view, reading);
  expect(cache.take("0", "正文草稿")).toBeUndefined();
  expect(cache.take("1", "正文草稿")).toBeDefined();
  const disabled = useEditorTabCache(0);
  disabled.save("a.md", view, reading);
  expect(disabled.take("a.md", "正文草稿")).toBeUndefined();
});

/** 关闭、删除、改名只保留仍打开的路径，卸载清空全部会话状态。 */
test("标签生命周期淘汰缓存且clear清空会话", () => {
  const cache = useEditorTabCache();
  const view = editor();
  for (const path of ["closed.md", "deleted.md", "renamed.md", "open.md"]) cache.save(path, view, reading);
  cache.retain(["new-name.md", "open.md"]);
  for (const path of ["closed.md", "deleted.md", "renamed.md", "new-name.md"]) {
    expect(cache.take(path, "正文草稿")).toBeUndefined();
  }
  expect(cache.take("open.md", "正文草稿")).toBeDefined();
  cache.save("open.md", view, reading);
  cache.clear();
  expect(cache.take("open.md", "正文草稿")).toBeUndefined();
});
