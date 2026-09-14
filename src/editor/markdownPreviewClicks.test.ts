import { markdown, markdownLanguage } from "@codemirror/lang-markdown";
import { EditorState, type Extension } from "@codemirror/state";
import { EditorView } from "@codemirror/view";
import { describe, expect, test, vi } from "vitest";
import { cardAnchorPlugin } from "./cardAnchors";
import { codeBlockPreview } from "./codeBlocks";
import { hideMarkersPlugin } from "./hideMarkers";
import { markdownImagePreview } from "./markdownImages";
import { markdownTablePreview } from "./markdownTables";

/** 与正式编辑器使用同一解析配置，覆盖预览 widget 的点击行为。 */
function createView(content: string, extra: Extension[] = []): EditorView {
  return new EditorView({
    parent: document.body,
    state: EditorState.create({ doc: content, extensions: [
      markdown({ base: markdownLanguage }), hideMarkersPlugin, ...extra,
    ] }),
  });
}

/** 按可见文字找预览内的控件，避免依赖按钮顺序。 */
function buttonByText(view: EditorView, text: string): HTMLElement {
  const button = [...view.dom.querySelectorAll("button")].find((item) => item.textContent === text);
  if (!button) throw new Error(`找不到按钮：${text}`);
  return button as HTMLElement;
}

describe("预览 widget 的点击入口", () => {
  test("点代码块正文进入编辑，光标落在代码体起点且不改写文档", () => {
    const doc = "前文\n\n```ts\nconst a = 1;\n```\n\n后文";
    const view = createView(doc, [codeBlockPreview]);
    try {
      const body = view.dom.querySelector(".qc-code-block pre") as HTMLElement;
      expect(body).toBeTruthy();
      body.click();
      expect(view.dom.querySelector(".qc-code-block")).toBeNull();
      expect(view.state.selection.main.head).toBe(doc.indexOf("const a = 1;"));
      expect(view.state.doc.toString()).toBe(doc);
    } finally { view.destroy(); }
  });

  test("点代码块工具栏按钮不会顺带把光标切进代码", () => {
    const doc = "前文\n\n```ts\nconst a = 1;\n```\n\n后文";
    const view = createView(doc, [codeBlockPreview]);
    try {
      buttonByText(view, "自动换行").click();
      expect(view.state.selection.main.head).toBe(0);
      expect(view.dom.querySelector(".qc-code-block")).not.toBeNull();
    } finally { view.destroy(); }
  });

  test("点图片预览回到图片语法，光标落到图片起始处", () => {
    const doc = "前文\n\n![图](a.png)\n\n后文";
    const view = createView(doc, [markdownImagePreview("note.md", async () => "data:image/png;base64,")]);
    try {
      const preview = view.dom.querySelector(".qc-image-preview") as HTMLElement;
      expect(preview).toBeTruthy();
      preview.click();
      expect(view.state.selection.main.head).toBe(doc.indexOf("![图]"));
    } finally { view.destroy(); }
  });

  test("图片解码完成后通知编辑器重新测绘，避免点击落点整体偏移", async () => {
    const doc = "前文\n\n![图](a.png)\n\n后文";
    const view = createView(doc, [markdownImagePreview("note.md", async () => "data:image/png;base64,AA")]);
    try {
      // 等待异步解析落地：widget 会带着真实图片重建。
      await vi.waitFor(() => {
        expect(view.dom.querySelector(".qc-image-preview img")).toBeTruthy();
      });
      // 加载完成后 widget 必须仍然在 DOM 里：曾经因为与隐藏标记装饰重叠而渲染成空节点。
      expect(view.dom.querySelector(".qc-image-preview")?.className).toContain("is-loaded");
      const image = view.dom.querySelector(".qc-image-preview img") as HTMLImageElement;
      const measure = vi.spyOn(view, "requestMeasure");
      image.dispatchEvent(new Event("load"));
      expect(measure).toHaveBeenCalled();
    } finally { view.destroy(); }
  });

  test("预览与源码互换后图片行不会塌成空节点", async () => {
    const doc = "前文\n\n![图](a.png)\n\n后文";
    const view = createView(doc, [markdownImagePreview("note.md", async () => "data:image/png;base64,AA")]);
    try {
      await vi.waitFor(() => {
        expect(view.dom.querySelector(".qc-image-preview img")).toBeTruthy();
      });
      // 点进源码再离开：两次切换都必须留下完整内容，否则行高变化会让后续点击落点整体偏移。
      (view.dom.querySelector(".qc-image-preview") as HTMLElement).click();
      expect(view.dom.textContent).toContain("![图](a.png)");
      view.dispatch({ selection: { anchor: 0 } });
      expect(view.dom.querySelector(".qc-image-preview")?.className).toContain("is-loaded");
    } finally { view.destroy(); }
  });

  test("光标在同一行但不碰图片时预览保持显示，碰到图片才回到源码", () => {
    const doc = "前文 ![图](a.png) 后文";
    const view = createView(doc, [markdownImagePreview("note.md", async () => "data:image/png;base64,")]);
    try {
      view.dispatch({ selection: { anchor: 1 } });
      expect(view.dom.querySelector(".qc-image-preview")).not.toBeNull();
      view.dispatch({ selection: { anchor: doc.indexOf("![") + 3 } });
      expect(view.dom.querySelector(".qc-image-preview")).toBeNull();
    } finally { view.destroy(); }
  });

  test("无法预览的远程图片按源码显示，不再只剩 alt 文字", () => {
    const doc = "前文 ![远程](https://example.com/a.png) 后文";
    const view = createView(doc);
    try {
      expect(view.dom.textContent).toBe(doc);
    } finally { view.destroy(); }
  });

  test("点已拆卡徽章触发打开卡片回调，且不移动光标", () => {
    const openCard = vi.fn();
    const view = createView("前文 ^qc-abc 后文", [cardAnchorPlugin(openCard)]);
    try {
      const chip = view.dom.querySelector(".qc-anchor-chip") as HTMLElement;
      expect(chip).toBeTruthy();
      chip.click();
      expect(openCard).toHaveBeenCalledExactlyOnceWith("abc");
      expect(view.state.selection.main.head).toBe(0);
    } finally { view.destroy(); }
  });

  test("表格单元格精确落点，边框留白回到表格源码", () => {
    const doc = "前文\n\n| A | B |\n| --- | --- |\n| x | y |\n\n后文";
    const view = createView(doc, [markdownTablePreview]);
    try {
      (view.dom.querySelector("td") as HTMLElement).click();
      expect(view.state.selection.main.head).toBe(doc.indexOf("x |"));
      // 光标移出表格后恢复预览，再点留白应回到表格起点。
      view.dispatch({ selection: { anchor: doc.length } });
      (view.dom.querySelector(".qc-table-preview") as HTMLElement).click();
      expect(view.state.selection.main.head).toBe(doc.indexOf("| A |"));
      expect(view.state.doc.toString()).toBe(doc);
    } finally { view.destroy(); }
  });
});

describe("标记按元素粒度显隐", () => {
  test("点普通文字不露出标记，光标进入元素才显示该元素的标记", () => {
    const doc = "**粗体** 后面普通文字";
    const view = createView(doc);
    try {
      view.dispatch({ selection: { anchor: doc.indexOf("后面") } });
      expect(view.dom.textContent).toBe("粗体 后面普通文字");
      view.dispatch({ selection: { anchor: doc.indexOf("粗体") } });
      expect(view.dom.textContent).toBe(doc);
    } finally { view.destroy(); }
  });

  test("LaTeX 与转义反斜杠逐字保留，不被当作文法改写", () => {
    const doc = "$$\\int_0^1 x^2\\,dx$$ 与 \\*不是斜体\\*";
    const view = createView(doc);
    try {
      view.dispatch({ selection: { anchor: doc.indexOf("斜") } });
      expect(view.dom.textContent).toBe(doc);
    } finally { view.destroy(); }
  });

  test("删除线标记与表格预览行为一致，按元素隐藏", () => {
    const doc = "前 ~~删除~~ 后";
    const view = createView(doc);
    try {
      view.dispatch({ selection: { anchor: doc.length } });
      expect(view.dom.textContent).toBe("前 删除 后");
    } finally { view.destroy(); }
  });
});
