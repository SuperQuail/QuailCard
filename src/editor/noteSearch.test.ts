import { history, undo } from "@codemirror/commands";
import { EditorState } from "@codemirror/state";
import { EditorView } from "@codemirror/view";
import { replaceAll, SearchQuery, setSearchQuery } from "@codemirror/search";
import { afterEach, expect, test } from "vitest";
import { noteSearchExtension } from "./noteSearch";

const views: EditorView[] = [];
/** 用真实 EditorView 验证按键、面板文案与替换命令，不 mock CodeMirror 自身。 */
function createEditor(doc: string, readOnly = false): EditorView {
  const view = new EditorView({
    state: EditorState.create({
      doc,
      extensions: [history(), EditorState.readOnly.of(readOnly), noteSearchExtension()],
    }),
    parent: document.body,
  });
  views.push(view);
  return view;
}

/** 释放编辑器与面板 DOM，避免残留元素干扰后续查询。 */
afterEach(() => { views.splice(0).forEach((view) => view.destroy()); });

/** 模拟 Ctrl+F：非 Mac 平台 Mod 映射为 Ctrl，事件必须落在 contentDOM 才会进入键位映射。 */
function pressFind(view: EditorView): void {
  view.contentDOM.dispatchEvent(new KeyboardEvent("keydown", {
    key: "f", ctrlKey: true, bubbles: true, cancelable: true,
  }));
}

/** 面板打开后才能证明快捷键与扩展都挂上了；同时锁定置顶位置。 */
test("Ctrl+F 在顶部打开查找面板", () => {
  const view = createEditor("alpha beta");
  pressFind(view);
  expect(view.dom.querySelector(".cm-panels-top .cm-search")).not.toBeNull();
  expect(view.dom.querySelector("[name=search]")).not.toBeNull();
});

/** 面板上的英文短语全部换成中文，含按钮、标签与关闭键的无障碍名称。 */
test("面板使用中文文案", () => {
  const view = createEditor("alpha beta");
  pressFind(view);
  const panel = view.dom.querySelector<HTMLElement>(".cm-search")!;
  const button = (name: string) => panel.querySelector(`button[name=${name}]`)?.textContent;
  expect(panel.querySelector("[name=search]")?.getAttribute("placeholder")).toBe("查找");
  expect(panel.querySelector("[name=replace]")?.getAttribute("placeholder")).toBe("替换");
  expect(button("next")).toBe("下一个");
  expect(button("prev")).toBe("上一个");
  expect(button("select")).toBe("全部");
  expect(button("replace")).toBe("替换");
  expect(button("replaceAll")).toBe("全部替换");
  expect(panel.textContent).toContain("区分大小写");
  expect(panel.textContent).toContain("正则表达式");
  expect(panel.textContent).toContain("整词");
  expect(panel.querySelector("button[name=close]")?.getAttribute("aria-label")).toBe("关闭");
});

/** 正则查询与全部替换要合成一次事务，撤销一次就回到原文。 */
test("正则全部替换生效且能一次撤销", () => {
  const view = createEditor("cat dog cat");
  view.dispatch({ effects: setSearchQuery.of(new SearchQuery({ search: "c.t", replace: "bird", regexp: true })) });
  expect(replaceAll(view)).toBe(true);
  expect(view.state.doc.toString()).toBe("bird dog bird");
  expect(undo(view)).toBe(true);
  expect(view.state.doc.toString()).toBe("cat dog cat");
});

/** 只读编辑器的替换由 CodeMirror 自身拦截：面板没有替换控件，命令也返回 false。 */
test("只读时不能替换", () => {
  const view = createEditor("cat", true);
  pressFind(view);
  const panel = view.dom.querySelector<HTMLElement>(".cm-search")!;
  expect(panel.querySelector("[name=replace]")).toBeNull();
  expect(panel.querySelector("[name=replaceAll]")).toBeNull();
  view.dispatch({ effects: setSearchQuery.of(new SearchQuery({ search: "cat", replace: "dog" })) });
  expect(replaceAll(view)).toBe(false);
  expect(view.state.doc.toString()).toBe("cat");
});
