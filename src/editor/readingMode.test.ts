import { markdown, markdownLanguage } from "@codemirror/lang-markdown";
import { Compartment, EditorSelection, EditorState } from "@codemirror/state";
import { EditorView } from "@codemirror/view";
import { afterEach, describe, expect, test, vi } from "vitest";
import { mathExtension } from "../markdown/parser";
import { codeBlockPreview } from "./codeBlocks";
import { hideMarkersPlugin } from "./hideMarkers";
import { markdownImagePreview } from "./markdownImages";
import { markdownListGlyphs } from "./markdownLists";
import { markdownMathPreview } from "./markdownMath";
import { markdownHorizontalRules } from "./markdownRules";
import { markdownTablePreview } from "./markdownTables";
import { readingMode, readingModeChanged, sourceSelectionTouches } from "./readingMode";

const views: EditorView[] = [];

/** 释放真实编辑器和测试替身，避免测绘任务或图片响应污染后续用例。 */
afterEach(() => {
  for (const view of views.splice(0)) view.destroy();
  vi.restoreAllMocks();
});

/** 只重配阅读 facet，不借助文档、选区或重建编辑器触发装饰刷新。 */
function createView(doc: string, anchor = 0, initialReading = false) {
  const mode = new Compartment();
  const view = new EditorView({
    parent: document.body,
    state: EditorState.create({
      doc, selection: { anchor }, extensions: [
        markdown({ base: markdownLanguage, extensions: [mathExtension] }),
        mode.of(readingMode.of(initialReading)), hideMarkersPlugin, codeBlockPreview,
        markdownTablePreview, markdownMathPreview, markdownListGlyphs, markdownHorizontalRules,
        // 使用本地资源替身，仍走真实插件的异步加载和装饰路径。
        markdownImagePreview("note.md", async () => "data:image/png;base64,AA"),
      ],
    }),
  });
  views.push(view);
  return { view,
    /** 保持选区不变，模拟上层阅读开关的 Compartment 契约。 */
    setReading(value: boolean): void { view.dispatch({ effects: mode.reconfigure(readingMode.of(value)) }); },
  };
}

/** 查询失败立即报错，避免可选链让交互测试空跑。 */
function element(view: EditorView, selector: string): HTMLElement {
  const node = view.dom.querySelector<HTMLElement>(selector);
  if (!node) throw new Error(`缺少预览元素：${selector}`);
  return node;
}

/** 用文字定位真实工具栏，验证按钮可用性而不依赖按钮排序。 */
function button(view: EditorView, text: string): HTMLButtonElement {
  const node = [...view.dom.querySelectorAll("button")].find((item) => item.textContent === text);
  if (!node) throw new Error(`缺少按钮：${text}`);
  return node;
}

const previews = [
  { name: "代码", source: "```ts\nconst x = 1;\n```", token: "const", selector: ".qc-code-block" },
  { name: "表格", source: "| A | B |\n| --- | --- |\n| **值** | 2 |", token: "值", selector: ".qc-table-preview" },
  { name: "行内公式", source: "$x^2$", token: "$", selector: ".qc-math .katex" },
  { name: "展示公式", source: "$$E=mc^2$$", token: "E", selector: ".qc-math .katex-display" },
  { name: "列表", source: "- 列表项", token: "-", selector: ".qc-bullet" },
  { name: "任务列表", source: "- [ ] 任务", token: "[", selector: ".qc-bullet" },
  { name: "分割线", source: "---", token: "-", selector: ".qc-rule" },
  { name: "图片", source: "![图](a.png)", token: "!", selector: ".qc-image-preview" },
];

/** 同一组真实插件必须在光标相交时保持预览，并在切回编辑后恢复源码。 */
describe("阅读模式预览切换", () => {
  test.each(previews)("$name：光标相交仍渲染，点击不派发选区或 focus，切回恢复", ({ source, token, selector }) => {
    const doc = `前文\n\n${source}\n\n后文`;
    const anchor = doc.indexOf(token) + 1;
    const { view, setReading } = createView(doc, anchor);
    expect(view.dom.querySelector(selector)).toBeNull();
    setReading(true);
    const preview = element(view, selector);
    const dispatch = vi.spyOn(view, "dispatch");
    const focus = vi.spyOn(view, "focus");
    preview.click();
    expect(dispatch).not.toHaveBeenCalled();
    expect(focus).not.toHaveBeenCalled();
    expect(view.state.selection.main.head).toBe(anchor);
    // 阅读时改变选区也不能把预览变回源码。
    view.dispatch({ selection: { anchor: 0, head: doc.length } });
    expect(view.dom.querySelector(selector)).not.toBeNull();
    setReading(false);
    expect(view.dom.querySelector(selector)).toBeNull();
    expect(view.state.doc.toString()).toBe(doc);
  });

  test.each(["**粗体**", "# 标题", "``行内代码``"])("%s：光标所在元素的标记随 facet 隐藏和恢复", (source) => {
    const { view, setReading } = createView(source, 2);
    expect(view.dom.textContent).toContain(source);
    setReading(true);
    expect(view.dom.textContent).not.toContain(source);
    expect(view.dom.textContent).toContain(source.replace(/[*#`]/g, "").trim());
    setReading(false);
    expect(view.dom.textContent).toContain(source);
  });

  test("初始阅读配置也渲染光标所在图片，异步加载后仍不能回源码", async () => {
    const { view, setReading } = createView("![图](a.png)", 2, true);
    await vi.waitFor(() => expect(view.dom.querySelector(".qc-image-preview img")).not.toBeNull());
    const dispatch = vi.spyOn(view, "dispatch");
    const focus = vi.spyOn(view, "focus");
    element(view, "img").click();
    expect(dispatch).not.toHaveBeenCalled();
    expect(focus).not.toHaveBeenCalled();
    setReading(false);
    expect(view.dom.textContent).toContain("![图](a.png)");
  });

  test.each(previews)("$name：已有预览 DOM 随模式改变点击权限，切回恢复入口", ({ source, selector }) => {
    const doc = `前文\n\n${source}`;
    const { view, setReading } = createView(doc);
    element(view, selector);
    setReading(true);
    const dispatch = vi.spyOn(view, "dispatch");
    const focus = vi.spyOn(view, "focus");
    element(view, selector).click();
    expect(dispatch).not.toHaveBeenCalled();
    expect(focus).not.toHaveBeenCalled();
    setReading(false);
    element(view, selector).click();
    expect(view.state.selection.main.head).toBeGreaterThan(0);
    expect(view.dom.querySelector(selector)).toBeNull();
    expect(focus).toHaveBeenCalled();
    expect(view.state.doc.toString()).toBe(doc);
  });
});

/** 不只检测源码显隐，还验证仍可见的工具栏和表格旧 DOM 不残留编辑权限。 */
describe("阅读模式预览操作", () => {
  test("代码仅保留复制和换行，模式切换重建工具栏并恢复编辑按钮", async () => {
    const doc = "前文\n\n```ts\nconst x = 1;\n```";
    const { view, setReading } = createView(doc);
    const oldEdit = button(view, "编辑");
    const copy = vi.fn().mockResolvedValue(undefined);
    const clipboard = Object.getOwnPropertyDescriptor(navigator, "clipboard");
    Object.defineProperty(navigator, "clipboard", { configurable: true, value: { writeText: copy } });
    try {
      setReading(true);
      expect([...view.dom.querySelectorAll("button")].map((node) => node.textContent)).toEqual(["复制", "自动换行"]);
      const dispatch = vi.spyOn(view, "dispatch");
      const focus = vi.spyOn(view, "focus");
      oldEdit.click();
      button(view, "复制").click();
      await vi.waitFor(() => expect(button(view, "已复制")).toBeTruthy());
      expect(copy).toHaveBeenCalledWith("const x = 1;");
      button(view, "自动换行").click();
      expect(element(view, "pre").classList.contains("is-wrapped")).toBe(true);
      expect(button(view, "横向滚动").getAttribute("aria-pressed")).toBe("true");
      button(view, "横向滚动").click();
      expect(element(view, "pre").classList.contains("is-wrapped")).toBe(false);
      expect(dispatch).not.toHaveBeenCalled();
      expect(focus).not.toHaveBeenCalled();
      setReading(false);
      button(view, "编辑").click();
      expect(view.state.selection.main.head).toBe(doc.indexOf("const"));
      expect(view.dom.querySelector(".qc-code-block")).toBeNull();
      expect(view.state.doc.toString()).toBe(doc);
    } finally {
      if (clipboard) Object.defineProperty(navigator, "clipboard", clipboard);
      else Reflect.deleteProperty(navigator, "clipboard");
    }
  });

  test("表格阅读单元格不接受 Tab、focus、点击或键盘编辑，切回恢复", () => {
    const doc = "前文\n\n| A | B |\n| :--- | ---: |\n| **值** | 2 |";
    const { view, setReading } = createView(doc);
    const oldCell = element(view, "td");
    expect(oldCell.tabIndex).toBe(0);
    setReading(true);
    const cell = element(view, "td");
    expect(cell).not.toBe(oldCell);
    expect(cell.querySelector("strong")?.textContent).toBe("值");
    expect(element(view, "th").style.textAlign).toBe("left");
    const dispatch = vi.spyOn(view, "dispatch");
    const focus = vi.spyOn(view, "focus");
    for (const node of view.dom.querySelectorAll<HTMLElement>("td, th")) {
      expect(node.hasAttribute("tabindex")).toBe(false);
      expect(node.tabIndex).toBe(-1);
      expect(node.title).toBe("");
      node.focus();
      expect(document.activeElement).not.toBe(node);
      node.click();
      for (const key of ["Enter", " ", "Tab"]) node.dispatchEvent(new KeyboardEvent("keydown", { key, bubbles: true }));
    }
    oldCell.click();
    expect(dispatch).not.toHaveBeenCalled();
    expect(focus).not.toHaveBeenCalled();
    setReading(false);
    const editable = element(view, "td");
    expect(editable.tabIndex).toBe(0);
    expect(editable.title).toBe("点击编辑此单元格");
    editable.dispatchEvent(new KeyboardEvent("keydown", { key: "Enter", bubbles: true }));
    expect(view.state.selection.main.head).toBe(doc.indexOf("**值**"));
    expect(view.dom.querySelector("table")).toBeNull();
    expect(view.state.doc.toString()).toBe(doc);
  });

  test("facet 默认编辑、任一 true 启用阅读，选区边界和多选区契约保持不变", () => {
    const state = EditorState.create({ doc: "0123456789",
      selection: EditorSelection.create([EditorSelection.cursor(1), EditorSelection.range(5, 8)]),
      extensions: [EditorState.allowMultipleSelections.of(true)],
    });
    expect(state.facet(readingMode)).toBe(false);
    expect(sourceSelectionTouches(state, 2, 4)).toBe(false);
    expect(sourceSelectionTouches(state, 2, 5)).toBe(true);
    expect(sourceSelectionTouches(state, 1, 1)).toBe(true);
    const read = EditorState.create({ doc: state.doc, selection: state.selection,
      extensions: [readingMode.of(false), readingMode.of(true)],
    });
    expect(read.facet(readingMode)).toBe(true);
    expect(sourceSelectionTouches(read, 0, 9)).toBe(false);
    expect(readingModeChanged(state, read)).toBe(true);
    expect(readingModeChanged(state, state)).toBe(false);
  });
});
