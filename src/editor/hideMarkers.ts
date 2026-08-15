import { syntaxTree } from "@codemirror/language";
import { RangeSetBuilder } from "@codemirror/state";
import { Decoration, DecorationSet, EditorView, ViewPlugin, ViewUpdate } from "@codemirror/view";

/** 需要隐藏的 Markdown 语法标记节点名。 */
const HIDDEN_MARK_NODES = new Set(["HeaderMark", "EmphasisMark", "QuoteMark", "InlineCode"]);

/** 为隐藏语法标记构建装饰：光标所在行的标记保持可见以便编辑。 */
function buildHiddenMarks(view: EditorView): DecorationSet {
  const builder = new RangeSetBuilder<Decoration>();
  const cursorLine = view.state.doc.lineAt(view.state.selection.main.head).number;
  syntaxTree(view.state).iterate({
    enter(node) {
      if (!HIDDEN_MARK_NODES.has(node.name)) {
        return;
      }
      // 光标在这一行时不隐藏，方便直接修改标记。
      if (view.state.doc.lineAt(node.from).number === cursorLine) {
        return;
      }
      builder.add(node.from, node.to, Decoration.replace({}));
    },
  });
  return builder.finish();
}

/** Live Preview 式语法隐藏插件：非光标行替换 `#`、`**` 等标记为零宽。 */
export const hideMarkersPlugin = ViewPlugin.fromClass(
  class {
    decorations: DecorationSet;

    constructor(view: EditorView) {
      this.decorations = buildHiddenMarks(view);
    }

    update(update: ViewUpdate): void {
      if (update.docChanged || update.selectionSet || update.viewportChanged) {
        this.decorations = buildHiddenMarks(update.view);
      }
    }
  },
  { decorations: (plugin) => plugin.decorations },
);
