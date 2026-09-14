import { syntaxTree } from "@codemirror/language";
import type { EditorState, Range } from "@codemirror/state";
import { Decoration, type DecorationSet, EditorView, ViewPlugin, type ViewUpdate, WidgetType } from "@codemirror/view";
import { focusSourceOnClick } from "./sourceOnClick";
import { readingModeChanged, sourceSelectionTouches } from "./readingMode";

/** 分割线的源码范围。 */
export interface HorizontalRuleRef {
  from: number;
  to: number;
}

/** 收集分割线节点；Setext 标题的下划线是 HeaderMark，不会出现在这里。 */
export function collectHorizontalRules(state: EditorState): HorizontalRuleRef[] {
  const rules: HorizontalRuleRef[] = [];
  syntaxTree(state).iterate({
    enter(node) {
      if (node.name === "HorizontalRule") {
        rules.push({ from: node.from, to: node.to });
      }
    },
  });
  return rules;
}

/** 分割线 Widget：整行画一条横线，点击回到源码。 */
class RuleWidget extends WidgetType {
  /** 保留源码位置快照，确保预览与编辑入口指向同一内容。 */
  constructor(private readonly from: number) { super(); }

  /** 位置变化必须重建，阅读权限由点击时的最新状态决定。 */
  eq(other: RuleWidget): boolean {
    return other.from === this.from;
  }

  /** 复用既有主题结构，点击权限由当前模式统一判断。 */
  toDOM(view: EditorView): HTMLElement {
    const span = document.createElement("span");
    span.className = "qc-rule";
    focusSourceOnClick(span, view, () => this.from);
    return span;
  }

  /** 点击由 focusSourceOnClick 处理，编辑器不再把它当作光标定位的一部分。 */
  ignoreEvent(): boolean { return true; }
}

/** 分割线插件：`---`、`***`、`___` 与带空格写法统一渲染成横线，选区碰上时显示源码。 */
export const markdownHorizontalRules = ViewPlugin.fromClass(
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

    /** 分割线独占一行，因此"选区碰到"等价于"光标在这一行"。 */
    build(view: EditorView): DecorationSet {
      const ranges: Range<Decoration>[] = [];
      for (const rule of collectHorizontalRules(view.state)) {
        const active = sourceSelectionTouches(view.state, rule.from, rule.to);
        if (!active) {
          ranges.push(Decoration.replace({ widget: new RuleWidget(rule.from) }).range(rule.from, rule.to));
        }
      }
      return Decoration.set(ranges, true);
    }
  },
  { decorations: (plugin) => plugin.decorations },
);
