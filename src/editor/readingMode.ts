import { Facet, type EditorState } from "@codemirror/state";

/** 任一配置请求阅读模式即启用；未配置时保留原有编辑行为。 */
export const readingMode = Facet.define<boolean, boolean>({ combine: (values) => values.some(Boolean) });

/** 阅读时光标和选区不暴露源码；编辑时沿用包含边界的相交规则。 */
export function sourceSelectionTouches(state: EditorState, from: number, to: number): boolean {
  return !state.facet(readingMode)
    && state.selection.ranges.some((range) => range.from <= to && range.to >= from);
}

/** 单独重配 facet 不会改变文档或选区，装饰必须显式检测模式变化。 */
export function readingModeChanged(oldState: EditorState, newState: EditorState): boolean {
  return oldState.facet(readingMode) !== newState.facet(readingMode);
}
