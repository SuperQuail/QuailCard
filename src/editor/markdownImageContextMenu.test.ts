import { markdown } from "@codemirror/lang-markdown";
import { EditorState } from "@codemirror/state";
import { EditorView } from "@codemirror/view";
import { describe, expect, test, vi } from "vitest";
import { markdownImagePreview, type MarkdownImageOptions } from "./markdownImages";

/** 正文中唯一的本地图片：from/to 与载荷用于锁定右键回传契约。 */
const DOC = "前文 ![结构图](../attachments/map.webp) 后文";
const IMAGE = { from: 3, to: 34, alt: "结构图", source: "../attachments/map.webp" };

/** 默认解析结果：任意 Data URL 都能让 widget 进入 loaded。 */
const loadedResolver = async (): Promise<string> => "data:image/png;base64,cG5n";

/** 创建真实编辑器；光标停在文末，图片才不会被"选区回源"规则挡住。 */
function preview(
  options: MarkdownImageOptions = {},
  resolver: (notePath: string, source: string) => Promise<string> = loadedResolver,
): EditorView {
  const view = new EditorView({
    parent: document.body,
    state: EditorState.create({
      doc: DOC,
      extensions: [markdown(), markdownImagePreview("课程/笔记.md", resolver, options)],
    }),
  });
  view.dispatch({ selection: { anchor: DOC.length } });
  return view;
}

/** 等待异步图片结果触发的装饰刷新；右键必须命中已渲染的终态 DOM。 */
async function settle(): Promise<void> {
  await new Promise((resolve) => setTimeout(resolve, 0));
}

/** 按状态取预览外壳；状态类名由 widget 如实标记，用来确认钩子只挂在成功态。 */
function previewOf(view: EditorView, status: "loading" | "loaded" | "error"): HTMLElement {
  return view.dom.querySelector<HTMLElement>(`.qc-image-preview.is-${status}`)!;
}

describe("图片预览右键钩子", () => {
  test("右键已加载图片上报引用并拦截默认菜单", async () => {
    const onContextMenu = vi.fn();
    const view = preview({ onContextMenu });
    try {
      await settle();
      const bubbled = vi.fn();
      view.dom.addEventListener("contextmenu", bubbled);
      const event = new MouseEvent("contextmenu", { bubbles: true, cancelable: true });
      // dispatchEvent 返回 false 说明默认菜单已被取消，壳层不会再弹原生菜单。
      expect(previewOf(view, "loaded").dispatchEvent(event)).toBe(false);
      expect(onContextMenu).toHaveBeenCalledTimes(1);
      expect(onContextMenu).toHaveBeenCalledWith(IMAGE, event);
      expect(event.defaultPrevented).toBe(true);
      expect(bubbled).not.toHaveBeenCalled();
    } finally { view.destroy(); }
  });

  test("加载中的图片不触发右键回调", () => {
    const onContextMenu = vi.fn();
    let finish!: (dataUrl: string) => void;
    const view = preview({ onContextMenu }, () => new Promise((resolve) => { finish = resolve; }));
    try {
      const event = new MouseEvent("contextmenu", { bubbles: true, cancelable: true });
      previewOf(view, "loading").dispatchEvent(event);
      expect(onContextMenu).not.toHaveBeenCalled();
      expect(event.defaultPrevented).toBe(false);
    } finally {
      finish("data:image/png;base64,cG5n");
      view.destroy();
    }
  });

  test("加载失败的图片不触发右键回调", async () => {
    const onContextMenu = vi.fn();
    const view = preview({ onContextMenu }, async () => { throw new Error("附件不存在"); });
    try {
      await settle();
      const event = new MouseEvent("contextmenu", { bubbles: true, cancelable: true });
      previewOf(view, "error").dispatchEvent(event);
      expect(onContextMenu).not.toHaveBeenCalled();
      expect(event.defaultPrevented).toBe(false);
    } finally { view.destroy(); }
  });

  test("未提供回调时右键保留默认菜单且不报错", async () => {
    const view = preview();
    try {
      await settle();
      const event = new MouseEvent("contextmenu", { bubbles: true, cancelable: true });
      expect(() => previewOf(view, "loaded").dispatchEvent(event)).not.toThrow();
      expect(event.defaultPrevented).toBe(false);
    } finally { view.destroy(); }
  });
});
