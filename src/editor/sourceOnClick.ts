import type { EditorView } from "@codemirror/view";
import { readingMode } from "./readingMode";

/** 预览内自己处理点击的控件；这些元素不做"回到源码"处理。 */
const INTERACTIVE = "button, a, input, select, textarea";

/**
 * 给预览 widget 挂上"点正文回到源码"的入口。
 * widget 默认吞掉编辑器事件（WidgetType.ignoreEvent），点击只能由 widget 自己处理；
 * skip 用于放行已经自行处理点击的区域，避免同一次点击被处理两次。
 */
export function focusSourceOnClick(
  container: HTMLElement,
  view: EditorView,
  resolvePosition: () => number,
  skip?: (target: HTMLElement) => boolean,
): void {
  container.addEventListener("click", (event) => {
    // 按当前状态判断，避免复用的 widget DOM 在模式切换后仍派发选区或抢焦点。
    if (view.state.facet(readingMode)) return;
    const target = event.target as HTMLElement | null;
    if (!target || target.closest(INTERACTIVE) || skip?.(target)) {
      return;
    }
    view.dispatch({ selection: { anchor: resolvePosition() }, scrollIntoView: true });
    view.focus();
  });
}
