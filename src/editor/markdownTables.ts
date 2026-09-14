import { syntaxTree } from "@codemirror/language";
import { StateField, type EditorState } from "@codemirror/state";
import { Decoration, EditorView, WidgetType, type DecorationSet } from "@codemirror/view";
import { PREVIEW_MARK_NODES } from "./markdownMarks";
import { focusSourceOnClick } from "./sourceOnClick";
import { readingMode, readingModeChanged, sourceSelectionTouches } from "./readingMode";

type Node = ReturnType<typeof syntaxTree>["topNode"];
const inlineTags: Record<string, string> = {
  StrongEmphasis: "strong", Emphasis: "em", InlineCode: "code", Strikethrough: "s",
};

/** 根据语法节点生成安全的行内内容，笔记中的 HTML 始终作为文本展示。 */
function appendInline(parent: HTMLElement, node: Node, state: EditorState): void {
  let position = node.from;
  for (let child = node.firstChild; child; child = child.nextSibling) {
    parent.append(document.createTextNode(state.doc.sliceString(position, child.from)));
    if (!PREVIEW_MARK_NODES.has(child.name)) {
      const tag = inlineTags[child.name];
      const target = tag ? parent.appendChild(document.createElement(tag)) : parent;
      appendInline(target, child, state);
    }
    position = child.to;
  }
  // 只还原表格必需的竖线转义，其余反斜杠（含 LaTeX 命令）逐字保留。
  parent.append(document.createTextNode(state.doc.sliceString(position, node.to).replace(/\\\|/g, "|")));
}

/** 表格只替换显示，点击单元格将光标放回对应源码，绝不改写笔记。 */
class TableWidget extends WidgetType {
  /** 保存原始语法节点，以便使用真实单元格边界和转义规则。 */
  constructor(private readonly node: Node, private readonly state: EditorState) { super(); }

  /** 内容、位置或模式变化后重建，避免旧 DOM 保留错误的点击位置和编辑入口。 */
  eq(other: TableWidget): boolean {
    return this.node.from === other.node.from && this.node.to === other.node.to && this.state.doc === other.state.doc
      && !readingModeChanged(this.state, other.state);
  }

  /** 使用原生表格语义与主题配色，并保留表头定义的列对齐。 */
  toDOM(view: EditorView): HTMLElement {
    const wrapper = document.createElement("div");
    wrapper.className = "qc-table-preview";
    const table = wrapper.appendChild(document.createElement("table"));
    const header = this.node.getChild("TableHeader")!;
    const delimiter = this.node.getChild("TableDelimiter")!;
    const alignments = this.state.doc.sliceString(delimiter.from, delimiter.to).trim()
      .replace(/^\|/, "").replace(/\|$/, "").split("|").map((cell) => {
        const value = cell.trim();
        return value.endsWith(":") ? (value.startsWith(":") ? "center" : "right") : "left";
      });
    const columnCount = header.getChildren("TableCell").length;
    const head = table.appendChild(document.createElement("thead"));
    const body = table.appendChild(document.createElement("tbody"));
    for (const row of [header, ...this.node.getChildren("TableRow")]) {
      const tr = (row === header ? head : body).appendChild(document.createElement("tr"));
      const cells = row.getChildren("TableCell");
      for (let index = 0; index < columnCount; index++) {
        const cell = cells[index];
        const td = tr.appendChild(document.createElement(row === header ? "th" : "td"));
        td.style.textAlign = alignments[index] ?? "left";
        if (cell) appendInline(td, cell, this.state);
        // 阅读单元格只保留语义内容，不进入 Tab 顺序，也不挂编辑提示与事件。
        if (this.state.facet(readingMode)) continue;
        td.tabIndex = 0;
        td.title = "点击编辑此单元格";
        /** 点击和键盘激活都回到源码，由编辑器维护撤销与保存。 */
        const edit = (): void => {
          if (view.state.facet(readingMode)) return;
          view.dispatch({ selection: { anchor: cell?.from ?? row.to }, scrollIntoView: true });
          view.focus();
        };
        td.addEventListener("click", edit);
        td.addEventListener("keydown", (event) => {
          if (event.key === "Enter" || event.key === " ") { event.preventDefault(); edit(); }
        });
      }
    }
    // 单元格自己处理点击，边框与留白等其余区域回到表格源码。
    focusSourceOnClick(wrapper, view, () => this.node.from, (target) => Boolean(target.closest("td, th")));
    return wrapper;
  }

  /** 预览的交互由单元格处理，避免编辑器误算被替换区域的位置。 */
  ignoreEvent(): boolean { return true; }
}

/** 选区碰到表格时显示源码，其余表格由块级装饰渲染。 */
function tableDecorations(state: EditorState): DecorationSet {
  const ranges = [];
  const cursor = syntaxTree(state).cursor();
  do {
    if (cursor.name !== "Table") continue;
    const node = cursor.node;
    if (sourceSelectionTouches(state, node.from, node.to)) continue;
    ranges.push(Decoration.replace({ widget: new TableWidget(node, state), block: true }).range(node.from, node.to));
  } while (cursor.next());
  return Decoration.set(ranges, true);
}

/** 直接提供状态装饰，允许表格替换跨行内容并随解析更新。 */
export const markdownTablePreview = StateField.define<DecorationSet>({
  create: tableDecorations,
  /** 文档、选区或后台解析变化后重新判断源码与预览状态。 */
  update(value, transaction) {
    return transaction.docChanged || transaction.selection
      || readingModeChanged(transaction.startState, transaction.state)
      || syntaxTree(transaction.startState) !== syntaxTree(transaction.state) ? tableDecorations(transaction.state) : value;
  },
  provide: (field) => EditorView.decorations.from(field),
});

/** 表格复用现有阅读主题，宽内容在表格内部滚动。 */
export const markdownTableTheme = EditorView.theme({
  ".qc-table-preview": { margin: "12px 0", maxWidth: "100%", overflowX: "auto" },
  ".qc-table-preview table": { width: "100%", borderCollapse: "collapse", lineHeight: "1.65" },
  ".qc-table-preview th, .qc-table-preview td": { border: "1px solid var(--qc-hairline)", padding: "8px 12px", minWidth: "80px", verticalAlign: "top" },
  ".qc-table-preview th": { backgroundColor: "var(--qc-code-bg)", fontWeight: "600" },
  ".qc-table-preview code": { fontFamily: "var(--font-mono)", backgroundColor: "var(--qc-code-bg)", fontSize: "0.9em" },
  ".qc-table-preview :focus-visible": { outline: "2px solid var(--qc-accent)", outlineOffset: "-2px" },
});
