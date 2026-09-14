import { markdown, markdownLanguage } from "@codemirror/lang-markdown";
import { EditorState } from "@codemirror/state";
import { EditorView } from "@codemirror/view";
import { describe, expect, test } from "vitest";
import { collectListGlyphs, markdownListGlyphs } from "./markdownLists";

/** 与正式编辑器一致的语法配置。 */
function createView(content: string, anchor: number): EditorView {
  const view = new EditorView({
    parent: document.body,
    state: EditorState.create({ doc: content, extensions: [markdown({ base: markdownLanguage }), markdownListGlyphs] }),
  });
  view.dispatch({ selection: { anchor } });
  return view;
}

const doc = "正文\n\n- 一\n  - 二\n    - 三\n\n1. 有序";

describe("无序列表圆点", () => {
  test("按嵌套层级替换成圆点，有序列表保留序号", () => {
    const view = createView(doc, 0);
    try {
      expect([...view.dom.querySelectorAll(".qc-bullet")].map((element) => element.textContent)).toEqual(["•", "◦", "▪"]);
      expect(view.dom.textContent).toContain("1. 有序");
      expect(collectListGlyphs(view.state)).toEqual(["一", "二", "三"].map((label, index) => ({
        from: doc.indexOf(`- ${label}`),
        to: doc.indexOf(`- ${label}`) + 1,
        text: ["•", "◦", "▪"][index],
      })));
    } finally { view.destroy(); }
  });

  test("光标贴在标记上时显示源码，点圆点回到标记位置", () => {
    const view = createView(doc, doc.indexOf("- 一"));
    try {
      // 只有被光标贴住的那个标记显示源码，其余项仍然是圆点。
      expect(view.dom.textContent).toContain("- 一");
      expect([...view.dom.querySelectorAll(".qc-bullet")].map((element) => element.textContent)).toEqual(["◦", "▪"]);
      view.dispatch({ selection: { anchor: 0 } });
      (view.dom.querySelector(".qc-bullet") as HTMLElement).click();
      expect(view.state.selection.main.head).toBe(doc.indexOf("- 一"));
      expect([...view.dom.querySelectorAll(".qc-bullet")].map((element) => element.textContent)).toEqual(["◦", "▪"]);
    } finally { view.destroy(); }
  });

  test("任务项显示勾选框而不是圆点，源码仍可点回来", () => {
    const doc = "正文\n\n- [ ] 待办\n- [x] 完成";
    const view = createView(doc, 0);
    try {
      expect([...view.dom.querySelectorAll(".qc-bullet")].map((element) => element.textContent)).toEqual(["☐", "☑"]);
      expect(view.dom.textContent).not.toContain("[ ]");
      expect(view.dom.textContent).toContain("待办");
      expect(collectListGlyphs(view.state)).toEqual([
        { from: doc.indexOf("[ ]"), to: doc.indexOf("[ ]") + 3, text: "☐" },
        { from: doc.indexOf("[x]"), to: doc.indexOf("[x]") + 3, text: "☑" },
      ]);
    } finally { view.destroy(); }
  });

  test("星号与加号写法同样渲染成圆点", () => {
    const view = createView("正文\n\n* 星号\n+ 加号", 0);
    try {
      expect([...view.dom.querySelectorAll(".qc-bullet")].map((element) => element.textContent)).toEqual(["•", "•"]);
    } finally { view.destroy(); }
  });
});
