import { markdown, markdownLanguage } from "@codemirror/lang-markdown";
import { EditorState } from "@codemirror/state";
import { EditorView } from "@codemirror/view";
import { afterEach, expect, test, vi } from "vitest";
import { remeasureOnContentResize } from "./contentResize";

afterEach(() => { vi.unstubAllGlobals(); vi.restoreAllMocks(); });

/** 记录 ResizeObserver 回调，便于手动模拟内容尺寸变化。 */
class FakeResizeObserver {
  static callbacks: Array<() => void> = [];
  constructor(callback: () => void) { FakeResizeObserver.callbacks.push(callback); }
  observe(): void {}
  disconnect(): void {}
}

/** 与正式编辑器一致的语法配置。 */
function createView(): EditorView {
  return new EditorView({
    parent: document.body,
    state: EditorState.create({
      doc: "内容",
      extensions: [markdown({ base: markdownLanguage }), remeasureOnContentResize],
    }),
  });
}

test("内容高度在测量之外变化时重新测绘，避免点击落点整体偏移", () => {
  FakeResizeObserver.callbacks = [];
  vi.stubGlobal("ResizeObserver", FakeResizeObserver);
  const view = createView();
  try {
    // CodeMirror 自己也为滚动容器注册了一个观察者，所以回调不止一个。
    expect(FakeResizeObserver.callbacks.length).toBeGreaterThan(0);
    const measure = vi.spyOn(view, "requestMeasure");
    FakeResizeObserver.callbacks.forEach((notify) => notify());
    // 只有本插件的回调会同步请求测绘；CM 自己的回调走 50ms 定时器。
    expect(measure).toHaveBeenCalledTimes(1);
  } finally { view.destroy(); }
});

test("没有 ResizeObserver 的环境（例如 jsdom）不报错", () => {
  vi.stubGlobal("ResizeObserver", undefined);
  const view = createView();
  try {
    expect(view.dom.textContent).toContain("内容");
  } finally { view.destroy(); }
});
