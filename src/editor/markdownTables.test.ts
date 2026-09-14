import { markdown, markdownLanguage } from "@codemirror/lang-markdown";
import { EditorState } from "@codemirror/state";
import { EditorView } from "@codemirror/view";
import { describe, expect, test } from "vitest";
import { hideMarkersPlugin } from "./hideMarkers";
import { markdownTablePreview } from "./markdownTables";

/** 与正式编辑器使用同一语法配置，覆盖标记隐藏扩展的兼容性。 */
function createView(content: string): EditorView {
  return new EditorView({
    parent: document.body,
    state: EditorState.create({ doc: content, extensions: [
      markdown({ base: markdownLanguage }), markdownTablePreview, hideMarkersPlugin,
    ] }),
  });
}

describe("Markdown 表格预览", () => {
  test("渲染截图形式的表格、行内格式与列对齐，并可进入编辑后恢复预览", () => {
    const doc = "标题\n\n| Twine 概念 | Godot 中的对应概念 | 说明 |\n| :--- | :---: | ---: |\n| **Passage** | `Button` | 文本 |\n\n后文";
    const view = createView(doc);
    try {
      expect(view.dom.querySelectorAll("th")).toHaveLength(3);
      expect(view.dom.querySelector("td strong")?.textContent).toBe("Passage");
      expect(view.dom.querySelector("td code")?.textContent).toBe("Button");
      expect([...view.dom.querySelectorAll("th")].map((cell) => cell.style.textAlign)).toEqual(["left", "center", "right"]);
      (view.dom.querySelector("td") as HTMLElement).click();
      expect(view.dom.querySelector("table")).toBeNull();
      expect(view.state.selection.main.head).toBe(doc.indexOf("**Passage**"));
      view.dispatch({ selection: { anchor: doc.length } });
      expect(view.dom.querySelector("table")).not.toBeNull();
      expect(view.state.doc.toString()).toBe(doc);
    } finally { view.destroy(); }
  });

  test("支持省略外侧竖线、转义竖线和空单元格，HTML 不执行", () => {
    const view = createView("前文\n\nA | B\n--- | ---\na\\|b | <img src=x onerror=alert(1)>\nonly |\n");
    try {
      expect([...view.dom.querySelectorAll("td")].map((cell) => cell.textContent?.trim()))
        .toEqual(["a|b", "<img src=x onerror=alert(1)>", "only", ""]);
      expect(view.dom.querySelector("img")).toBeNull();
    } finally { view.destroy(); }
  });

  test("源码态保留单元格里的行内标记，不再显示被吃掉的假源码", () => {
    const doc = "标题\n\n| 概念 | 说明 |\n| --- | --- |\n| **Passage** | 文本 |\n\n后文";
    const view = createView(doc);
    try {
      // 光标停在另一个单元格：此时表格整体是源码态，粗体标记必须原样可见。
      view.dispatch({ selection: { anchor: doc.indexOf("文本") } });
      expect(view.dom.querySelector("table")).toBeNull();
      expect(view.dom.textContent).toContain("**Passage**");
    } finally { view.destroy(); }
  });

  test("围栏内的表格和没有分隔行的普通文本不变成表格", () => {
    const view = createView("前文\n\n```md\n| A | B |\n| --- | --- |\n```\n\nA | B\n普通文本\n");
    try { expect(view.dom.querySelector("table")).toBeNull(); }
    finally { view.destroy(); }
  });
});
