import { syntaxTree } from "@codemirror/language";
import { RangeSetBuilder } from "@codemirror/state";
import { Decoration, DecorationSet, EditorView, ViewPlugin, ViewUpdate } from "@codemirror/view";
import { BODY_MARK_NODES } from "./markdownMarks";
import { readingModeChanged, sourceSelectionTouches } from "./readingMode";

type Node = ReturnType<typeof syntaxTree>["topNode"];

/**
 * 该节点是否位于由预览 widget 整体接管的块内。
 * 图片与表格的整块都被 widget 替换，内部标记若同时被隐藏，两套替换装饰范围重叠，
 * 会让 widget 在重渲染时变成空节点（整行高度塌陷），源码态下还会看到"标记被吃掉"的假源码。
 */
function insideWidgetBlock(node: Node): boolean {
  for (let parent = node.parent; parent; parent = parent.parent) {
    if (parent.name === "Image" || parent.name === "Table") {
      return true;
    }
  }
  return false;
}

/**
 * 为隐藏语法标记构建装饰。
 * 判定按元素粒度：点普通文字不会露出任何标记，点进粗体/链接等元素才显示该元素的标记，
 * 因此不会因为隐藏标记的显隐让整行文字左右跳动。
 */
function buildHiddenMarks(view: EditorView): DecorationSet {
  const builder = new RangeSetBuilder<Decoration>();
  syntaxTree(view.state).iterate({
    enter(node) {
      if (BODY_MARK_NODES.has(node.name)) {
        if (insideWidgetBlock(node.node)) {
          return;
        }
        const owner = node.node.parent;
        if (owner && sourceSelectionTouches(view.state, owner.from, owner.to)) {
          return;
        }
        builder.add(node.from, node.to, Decoration.replace({}));
        return;
      }
      // 行内代码按反引号长度隐藏，多反引号也必须完整隐藏。
      if (node.name === "InlineCode") {
        if (insideWidgetBlock(node.node) || sourceSelectionTouches(view.state, node.from, node.to)) {
          return false;
        }
        const text = view.state.doc.sliceString(node.from, node.to);
        const width = text.match(/^`+/)?.[0].length ?? 0;
        if (width && text.endsWith("`".repeat(width)) && text.length > width * 2) {
          builder.add(node.from, node.from + width, Decoration.replace({}));
          builder.add(node.to - width, node.to, Decoration.replace({}));
        }
        return false;
      }
      // 转义反斜杠不隐藏：LaTeX 命令（\, \{ \_）依赖原样显示，隐藏会改写用户看到的公式。
    },
  });
  return builder.finish();
}

/** Live Preview 式语法隐藏插件：隐藏标记本身，正文内容始终可点可编辑。 */
export const hideMarkersPlugin = ViewPlugin.fromClass(
  class {
    decorations: DecorationSet;

    /** 初次挂载时也遵守当前模式，不依赖后续选区事务。 */
    constructor(view: EditorView) {
      this.decorations = buildHiddenMarks(view);
    }

    /** 模式重配没有选区变化，也必须刷新预览装饰。 */
    update(update: ViewUpdate): void {
      if (update.docChanged || update.selectionSet || update.viewportChanged
        || readingModeChanged(update.startState, update.state)) {
        this.decorations = buildHiddenMarks(update.view);
      }
    }
  },
  { decorations: (plugin) => plugin.decorations },
);
