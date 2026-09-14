import { markdown } from "@codemirror/lang-markdown";
import { EditorState } from "@codemirror/state";
import { EditorView } from "@codemirror/view";
import { describe, expect, test } from "vitest";
import { codeBlockPreview, collectCodeBlocks } from "./codeBlocks";

describe("围栏代码块", () => {
  test.each(["```", "~~~~", "````"])("识别 %s，保留语言、缩进、空行", (fence) => {
    const doc = `${fence}typescript\n  const n = 1;\n\n  // 注释\n${fence}`;
    const state = EditorState.create({ doc, extensions: [markdown()] });
    expect(collectCodeBlocks(state)[0]).toMatchObject({ language: "typescript", code: "  const n = 1;\n\n  // 注释" });
  });
  test("未闭合和空代码块不吞掉正文", () => {
    const state = EditorState.create({ doc: "```unknown\nvalue\n", extensions: [markdown()] });
    expect(collectCodeBlocks(state)[0].code).toBe("value\n");
    expect(collectCodeBlocks(EditorState.create({ doc: "```\n```", extensions: [markdown()] }))[0].code).toBe("");
  });
  test("光标离开显示整块预览，点击编辑恢复源码且文档不变", () => {
    const doc = "标题\n\n```ts\nconst x = 1;\n```\n";
    const view = new EditorView({ state: EditorState.create({ doc, extensions: [markdown(), codeBlockPreview] }), parent: document.body });
    try {
      expect(view.dom.querySelector(".qc-code-block pre")?.textContent).toBe("const x = 1;");
      const edit = [...view.dom.querySelectorAll("button")].find((button) => button.textContent === "编辑")!;
      edit.click();
      expect(view.dom.querySelector(".qc-code-block")).toBeNull();
      expect(view.dom.querySelectorAll(".qc-code-line").length).toBe(3);
      expect(view.state.doc.toString()).toBe(doc);
    } finally { view.destroy(); }
  });
});
