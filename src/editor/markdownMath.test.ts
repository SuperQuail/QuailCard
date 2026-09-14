import { markdown, markdownLanguage } from "@codemirror/lang-markdown";
import { EditorState } from "@codemirror/state";
import { EditorView } from "@codemirror/view";
import { describe, expect, test } from "vitest";
import { mathExtension } from "../markdown/parser";
import { collectMath, markdownMathPreview } from "./markdownMath";

/** 与正式编辑器一致的语法配置：同一个公式扩展，避免测试与产品行为分叉。 */
function createView(content: string, anchor: number): EditorView {
  const view = new EditorView({
    parent: document.body,
    state: EditorState.create({
      doc: content,
      extensions: [
        markdown({ base: markdownLanguage, extensions: [mathExtension] }),
        markdownMathPreview,
      ],
    }),
  });
  view.dispatch({ selection: { anchor } });
  return view;
}

describe("单行公式渲染", () => {
  test("行内公式渲染成 KaTeX，点击回到源码", () => {
    const doc = "前文 $x^2+y^2$ 后文";
    const view = createView(doc, doc.length);
    try {
      const formula = view.dom.querySelector(".qc-math") as HTMLElement;
      expect(formula).toBeTruthy();
      expect(formula.querySelector(".katex")).not.toBeNull();
      expect(formula.title).toBe("x^2+y^2");
      expect(view.dom.textContent).not.toContain("$x^2+y^2$");
      formula.click();
      expect(view.state.selection.main.head).toBe(doc.indexOf("$x^2"));
      expect(view.dom.querySelector(".qc-math")).toBeNull();
    } finally { view.destroy(); }
  });

  test("块级公式按 display 模式渲染并独占一行", () => {
    const doc = "前文\n\n$$E=mc^2$$";
    const view = createView(doc, 0);
    try {
      const formula = view.dom.querySelector(".qc-math") as HTMLElement;
      expect(formula.classList.contains("is-display")).toBe(true);
      expect(formula.querySelector(".katex-display")).not.toBeNull();
    } finally { view.destroy(); }
  });

  test("选区碰到公式时显示源码，光标离开后恢复渲染", () => {
    const doc = "前文 $x^2$ 后文";
    const view = createView(doc, doc.indexOf("x^2"));
    try {
      expect(view.dom.querySelector(".qc-math")).toBeNull();
      expect(view.dom.textContent).toContain("$x^2$");
      view.dispatch({ selection: { anchor: doc.length } });
      expect(view.dom.querySelector(".qc-math")).not.toBeNull();
    } finally { view.destroy(); }
  });

  test("普通美元符号、多行公式与反引号里的美元符号都不当成公式", () => {
    for (const doc of ["价格 $5 到 $10", "US$100 和 $200", "$$\nE=mc^2\n$$", "`$5` 反引号", "$x $ 结尾空格"]) {
      const view = createView(doc, doc.length);
      try {
        expect(collectMath(view.state)).toEqual([]);
        expect(view.dom.querySelector(".qc-math")).toBeNull();
      } finally { view.destroy(); }
    }
  });

  test("公式源码按原文交给 KaTeX，反斜杠命令不被改写", () => {
    const doc = "$\\frac{1}{2}$";
    const view = createView(doc, doc.length);
    try {
      expect(collectMath(view.state)).toEqual([{ from: 0, to: 13, source: "\\frac{1}{2}", display: false }]);
    } finally { view.destroy(); }
  });
});
