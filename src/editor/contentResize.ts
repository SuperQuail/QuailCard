import { EditorView, ViewPlugin } from "@codemirror/view";

/**
 * 内容高度在测量之外变化时重新测绘。
 * CodeMirror 只观察滚动容器（scrollDOM）的尺寸，图片解码、字体加载这类内容内部的高度变化
 * 它发现不了：高度表会停留在旧值，导致之后所有点击的落点整体偏移（点这一行、光标跑到下面几行）。
 */
export const remeasureOnContentResize = ViewPlugin.fromClass(
  class {
    private readonly observer: ResizeObserver | null;

    constructor(view: EditorView) {
      // jsdom 等环境没有 ResizeObserver，这类环境下直接跳过。
      this.observer = typeof ResizeObserver === "function"
        ? new ResizeObserver(() => view.requestMeasure())
        : null;
      this.observer?.observe(view.contentDOM);
    }

    /** 插件销毁时断开观察，避免引用已销毁的编辑器。 */
    destroy(): void {
      this.observer?.disconnect();
    }
  },
);
