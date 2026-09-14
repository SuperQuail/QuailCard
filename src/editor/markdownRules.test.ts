import { markdown, markdownLanguage } from "@codemirror/lang-markdown";
import { EditorState } from "@codemirror/state";
import { EditorView } from "@codemirror/view";
import { describe, expect, test } from "vitest";
import { collectHorizontalRules, markdownHorizontalRules } from "./markdownRules";

/** 与正式编辑器一致的语法配置。 */
function createView(content: string, anchor: number): EditorView {
  const view = new EditorView({
    parent: document.body,
    state: EditorState.create({ doc: content, extensions: [markdown({ base: markdownLanguage }), markdownHorizontalRules] }),
  });
  view.dispatch({ selection: { anchor } });
  return view;
}

describe("分割线渲染", () => {
  test("连字符、星号、下划线与带空格写法都渲染成横线", () => {
    for (const rule of ["---", "***", "___", "- - -"]) {
      const doc = `前文\n\n${rule}\n\n后文`;
      const view = createView(doc, 0);
      try {
        const from = doc.indexOf(rule);
        expect(collectHorizontalRules(view.state)).toEqual([{ from, to: from + rule.length }]);
        expect(view.dom.querySelector(".qc-rule")).not.toBeNull();
        expect(view.dom.textContent).not.toContain(rule);
      } finally { view.destroy(); }
    }
  });

  test("光标在这行时显示源码，点横线回到源码", () => {
    const doc = "前文\n\n---\n\n后文";
    const view = createView(doc, doc.indexOf("---"));
    try {
      expect(view.dom.querySelector(".qc-rule")).toBeNull();
      expect(view.dom.textContent).toContain("---");
      view.dispatch({ selection: { anchor: 0 } });
      (view.dom.querySelector(".qc-rule") as HTMLElement).click();
      expect(view.state.selection.main.head).toBe(doc.indexOf("---"));
      expect(view.dom.querySelector(".qc-rule")).toBeNull();
    } finally { view.destroy(); }
  });

  test("Setext 标题的下划线不会被当成横线", () => {
    const view = createView("标题\n---", 0);
    try {
      expect(collectHorizontalRules(view.state)).toEqual([]);
      expect(view.dom.querySelector(".qc-rule")).toBeNull();
    } finally { view.destroy(); }
  });
});
