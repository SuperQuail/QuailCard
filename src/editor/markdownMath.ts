import { syntaxTree } from "@codemirror/language";
import type { EditorState, Range } from "@codemirror/state";
import { Decoration, type DecorationSet, EditorView, ViewPlugin, type ViewUpdate, WidgetType } from "@codemirror/view";
import katex from "katex";
import { DISPLAY_MATH, INLINE_MATH, mathSource } from "../markdown/math";
import { focusSourceOnClick } from "./sourceOnClick";
import { readingModeChanged, sourceSelectionTouches } from "./readingMode";

/** 需要渲染的一段公式。 */
export interface MathNode {
  from: number;
  to: number;
  source: string;
  display: boolean;
}

/** 收集文档中的单行公式；多行 `$$` 不参与，因为解析器不认它们。 */
export function collectMath(state: EditorState): MathNode[] {
  const nodes: MathNode[] = [];
  const content = state.doc.toString();
  syntaxTree(state).iterate({
    enter(node) {
      if (node.name !== INLINE_MATH && node.name !== DISPLAY_MATH) {
        return;
      }
      nodes.push({
        from: node.from,
        to: node.to,
        source: mathSource(node.node, content),
        display: node.name === DISPLAY_MATH,
      });
      return false;
    },
  });
  return nodes;
}

/** 公式预览 Widget：用 KaTeX 渲染，渲染失败时退回源码，绝不留下空白块。 */
class MathWidget extends WidgetType {
  /** 保留源码位置快照，确保预览与编辑入口指向同一内容。 */
  constructor(private readonly math: MathNode) { super(); }

  /** 位置或内容变化必须重建，否则点击会跳错位置、显示会串公式。 */
  eq(other: MathWidget): boolean {
    return other.math.from === this.math.from
      && other.math.source === this.math.source
      && other.math.display === this.math.display;
  }

  /** 复用既有主题结构，点击权限由当前模式统一判断。 */
  toDOM(view: EditorView): HTMLElement {
    const wrapper = document.createElement("span");
    wrapper.className = `qc-math${this.math.display ? " is-display" : ""}`;
    wrapper.title = this.math.source;
    try {
      katex.render(this.math.source, wrapper, { displayMode: this.math.display, throwOnError: false });
    } catch {
      wrapper.textContent = this.math.source;
    }
    focusSourceOnClick(wrapper, view, () => this.math.from);
    return wrapper;
  }

  /** 点击由 focusSourceOnClick 处理，编辑器不再把它当作光标定位的一部分。 */
  ignoreEvent(): boolean { return true; }
}

/** 公式预览插件：与表格、代码块一致，选区碰到公式就显示源码。 */
export const markdownMathPreview = ViewPlugin.fromClass(
  class {
    decorations: DecorationSet;

    /** 初次挂载时也遵守当前模式，不依赖后续选区事务。 */
    constructor(view: EditorView) {
      this.decorations = this.build(view);
    }

    /** 模式重配没有选区变化，也必须刷新预览装饰。 */
    update(update: ViewUpdate): void {
      if (update.docChanged || update.selectionSet || update.viewportChanged
        || readingModeChanged(update.startState, update.state)) {
        this.decorations = this.build(update.view);
      }
    }

    /** 只替换选区未碰到的公式，光标进出公式时源码与渲染互换。 */
    build(view: EditorView): DecorationSet {
      const ranges: Range<Decoration>[] = [];
      for (const math of collectMath(view.state)) {
        const active = sourceSelectionTouches(view.state, math.from, math.to);
        if (!active) {
          ranges.push(Decoration.replace({ widget: new MathWidget(math) }).range(math.from, math.to));
        }
      }
      return Decoration.set(ranges, true);
    }
  },
  { decorations: (plugin) => plugin.decorations },
);
