import { markdown, markdownLanguage } from "@codemirror/lang-markdown";
import { EditorState } from "@codemirror/state";
import { EditorView } from "@codemirror/view";
import { describe, expect, test, vi } from "vitest";
import { wikilinkMarkdown } from "../markdown/wikilink";
import { readingMode } from "./readingMode";
import { wikilinkExtension, type WikiLinkOptions } from "./wikilinks";

/** 建视图：双链扩展的解析判定与打开动作全部由宿主注入。 */
function createView(doc: string, options: WikiLinkOptions, reading = false): EditorView {
  return new EditorView({
    parent: document.body,
    state: EditorState.create({
      doc,
      extensions: [
        markdown({ base: markdownLanguage, extensions: [wikilinkMarkdown] }),
        ...(reading ? [readingMode.of(true)] : []),
        wikilinkExtension(options),
      ],
    }),
  });
}

/** 只有 B 存在的解析回调，用于区分已解析/未解析样式。 */
function options(onOpen = vi.fn()): WikiLinkOptions {
  return { onOpen, isResolved: (target) => target === "B" };
}

describe("双链装饰扩展", () => {
  test("编辑态保留原文，并按是否可解析给不同样式", () => {
    const view = createView("见 [[B]] 与 [[C|显示]]", options());
    try {
      expect(view.dom.textContent).toContain("[[B]]");
      expect(view.dom.textContent).toContain("[[C|显示]]");
      expect(view.dom.querySelector(".qc-wikilink.is-resolved")?.textContent).toBe("[[B]]");
      expect(view.dom.querySelector(".qc-wikilink.is-unresolved")?.textContent).toBe("[[C|显示]]");
    } finally {
      view.destroy();
    }
  });

  test("编辑态 Ctrl/Cmd+点击才打开，普通点击只定位光标", () => {
    const onOpen = vi.fn();
    const view = createView("见 [[B]] 后文", options(onOpen));
    try {
      const link = view.dom.querySelector(".qc-wikilink.is-resolved") as HTMLElement;
      link.dispatchEvent(new MouseEvent("mousedown", { bubbles: true, button: 0 }));
      expect(onOpen).not.toHaveBeenCalled();
      link.dispatchEvent(new MouseEvent("mousedown", { bubbles: true, button: 0, ctrlKey: true }));
      expect(onOpen).toHaveBeenCalledWith("B");
    } finally {
      view.destroy();
    }
  });

  test("阅读态只显示显示名或目标名，不放原文", () => {
    const view = createView("见 [[B]] 与 [[C#小节|显示]]", options(), true);
    try {
      expect(view.dom.textContent).toContain("见 B 与 显示");
      expect(view.dom.textContent).not.toContain("[[");
      expect(view.dom.querySelector(".qc-wikilink-preview.is-resolved")?.textContent).toBe("B");
      expect(view.dom.querySelector(".qc-wikilink-preview.is-unresolved")?.textContent).toBe("显示");
    } finally {
      view.destroy();
    }
  });

  test("阅读态点击直接回传目标，内嵌形式带 is-embed", () => {
    const onOpen = vi.fn();
    const view = createView("![[B|图]]", { onOpen, isResolved: () => true }, true);
    try {
      const preview = view.dom.querySelector(".qc-wikilink-preview.is-embed") as HTMLElement;
      expect(preview.textContent).toBe("图");
      preview.click();
      expect(onOpen).toHaveBeenCalledWith("B");
    } finally {
      view.destroy();
    }
  });

  test("打开回调收到完整目标（含 #标题），由宿主决定跳转", () => {
    const onOpen = vi.fn();
    const view = createView("[[B#小节]]", { onOpen, isResolved: () => true }, true);
    try {
      (view.dom.querySelector(".qc-wikilink-preview") as HTMLElement).click();
      expect(onOpen).toHaveBeenCalledWith("B#小节");
    } finally {
      view.destroy();
    }
  });

  test("删除正文后装饰跟随语法树更新", () => {
    const view = createView("[[B]] 后文", options());
    try {
      view.dispatch({ changes: { from: 0, to: view.state.doc.length, insert: "普通文字" } });
      expect(view.dom.querySelector(".qc-wikilink")).toBeNull();
    } finally {
      view.destroy();
    }
  });
});
