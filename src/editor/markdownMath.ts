import { syntaxTree } from "@codemirror/language";
import { StateField, type EditorState, type Range } from "@codemirror/state";
import { Decoration, type DecorationSet, EditorView, WidgetType } from "@codemirror/view";
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
  /** 公式源码跨行：只能用块级装饰替换整行，普通 replace 会跨行报错。 */
  block: boolean;
}

/** 收集文档中的公式；多行 `$$` 围栏同样是一个 DisplayMath 节点，只是源码里带换行。 */
export function collectMath(state: EditorState): MathNode[] {
  const nodes: MathNode[] = [];
  const content = state.doc.toString();
  syntaxTree(state).iterate({
    enter(node) {
      if (node.name !== INLINE_MATH && node.name !== DISPLAY_MATH) {
        return;
      }
      const source = mathSource(node.node, content);
      nodes.push({
        from: node.from,
        to: node.to,
        source,
        display: node.name === DISPLAY_MATH,
        block: source.includes("\n"),
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

  /** 复用既有主题结构，点击权限由当前模式统一判断；跨行公式是块级装饰，必须用 div 承载。 */
  toDOM(view: EditorView): HTMLElement {
    const wrapper = document.createElement(this.math.block ? "div" : "span");
    wrapper.className = `qc-math${this.math.display ? " is-display" : ""}${this.math.block ? " is-block" : ""}`;
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

/** 选区碰到公式时显示源码，其余公式交给 KaTeX；跨行围栏只能用块级装饰替换整行。 */
function mathDecorations(state: EditorState): DecorationSet {
  const ranges: Range<Decoration>[] = [];
  for (const math of collectMath(state)) {
    if (sourceSelectionTouches(state, math.from, math.to)) continue;
    if (!math.block) {
      ranges.push(Decoration.replace({ widget: new MathWidget(math) }).range(math.from, math.to));
      continue;
    }
    // 块级装饰必须整行替换：起点在行首，终点在行尾。
    const first = state.doc.lineAt(math.from);
    const last = state.doc.lineAt(math.to);
    ranges.push(Decoration.replace({ widget: new MathWidget(math), block: true }).range(first.from, last.to));
  }
  return Decoration.set(ranges, true);
}

/** 块替换必须由状态装饰提供；ViewPlugin 装饰不允许替换整行。 */
export const markdownMathPreview = StateField.define<DecorationSet>({
  create: mathDecorations,
  /** 文档、选区、解析树或阅读模式变化后重新判断源码与预览。 */
  update(value, transaction) {
    return transaction.docChanged || transaction.selection
      || readingModeChanged(transaction.startState, transaction.state)
      || syntaxTree(transaction.startState) !== syntaxTree(transaction.state) ? mathDecorations(transaction.state) : value;
  },
  provide: (field) => EditorView.decorations.from(field),
});
