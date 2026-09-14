import { syntaxTree } from "@codemirror/language";
import type { EditorState, Range } from "@codemirror/state";
import { Decoration, type DecorationSet, EditorView, ViewPlugin, type ViewUpdate, WidgetType } from "@codemirror/view";
import { LIST_BULLETS } from "../markdown/model";
import { focusSourceOnClick } from "./sourceOnClick";
import { readingModeChanged, sourceSelectionTouches } from "./readingMode";

/** 一个待替换的列表标记。 */
export interface ListGlyph {
  from: number;
  to: number;
  text: string;
}

/**
 * 收集无序列表圆点与任务勾选框；有序列表保留序号，不参与替换。
 * 任务项由勾选框表示，因此不再额外画圆点，与显示侧保持一致。
 */
export function collectListGlyphs(state: EditorState): ListGlyph[] {
  const glyphs: ListGlyph[] = [];
  syntaxTree(state).iterate({
    enter(node) {
      if (node.name === "TaskMarker") {
        const checked = state.doc.sliceString(node.from + 1, node.from + 2) !== " ";
        glyphs.push({ from: node.from, to: node.to, text: checked ? "☑" : "☐" });
        return;
      }
      if (node.name !== "ListMark") return;
      const item = node.node.parent;
      if (!item || item.name !== "ListItem" || item.parent?.name !== "BulletList" || item.getChild("Task")) return;
      let depth = 0;
      for (let outer = item.parent.parent; outer; outer = outer.parent) {
        if (outer.name === "ListItem") depth++;
      }
      glyphs.push({ from: node.from, to: node.to, text: LIST_BULLETS[Math.min(depth, LIST_BULLETS.length - 1)] });
    },
  });
  return glyphs;
}

/** 列表标记 Widget：点击回到标记本身。 */
class ListGlyphWidget extends WidgetType {
  /** 保留源码位置快照，确保预览与编辑入口指向同一内容。 */
  constructor(private readonly glyph: ListGlyph) { super(); }

  /** 位置变化必须重建，阅读权限由点击时的最新状态决定。 */
  eq(other: ListGlyphWidget): boolean {
    return other.glyph.from === this.glyph.from && other.glyph.text === this.glyph.text;
  }

  /** 复用既有主题结构，点击权限由当前模式统一判断。 */
  toDOM(view: EditorView): HTMLElement {
    const span = document.createElement("span");
    span.className = "qc-bullet";
    span.textContent = this.glyph.text;
    focusSourceOnClick(span, view, () => this.glyph.from);
    return span;
  }

  /** 点击由 focusSourceOnClick 处理，编辑器不再把它当作光标定位的一部分。 */
  ignoreEvent(): boolean { return true; }
}

/**
 * 列表标记插件。
 * 只有选区贴到标记本身时才显示源码，因此列表在编辑状态下也保持列表外观，
 * 想改标记（例如把 - 换成 1.、把 [ ] 勾成 [x]）点一下符号即可。
 */
export const markdownListGlyphs = ViewPlugin.fromClass(
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

    /** 只替换未被选区碰到的标记。 */
    build(view: EditorView): DecorationSet {
      const ranges: Range<Decoration>[] = [];
      for (const glyph of collectListGlyphs(view.state)) {
        const active = sourceSelectionTouches(view.state, glyph.from, glyph.to);
        if (!active) {
          ranges.push(Decoration.replace({ widget: new ListGlyphWidget(glyph) }).range(glyph.from, glyph.to));
        }
      }
      return Decoration.set(ranges, true);
    }
  },
  { decorations: (plugin) => plugin.decorations },
);
