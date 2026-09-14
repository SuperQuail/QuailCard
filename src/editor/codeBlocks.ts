import { syntaxTree } from "@codemirror/language";
import { StateField, type EditorState, type Range } from "@codemirror/state";
import { Decoration, EditorView, WidgetType, type DecorationSet } from "@codemirror/view";
import { classHighlighter, highlightTree } from "@lezer/highlight";
import { focusSourceOnClick } from "./sourceOnClick";
import { readingMode, readingModeChanged, sourceSelectionTouches } from "./readingMode";

/** 代码区边界来自 Markdown 语法树，避免误识别字符串内的围栏。 */
export interface CodeBlock {
  from: number;
  to: number;
  bodyFrom: number;
  bodyTo: number;
  language: string;
  code: string;
}

/** 支持反引号、波浪线、多长度围栏和未闭合代码块。 */
export function collectCodeBlocks(state: EditorState): CodeBlock[] {
  const blocks: CodeBlock[] = [];
  syntaxTree(state).iterate({ enter(node) {
    if (node.name !== "FencedCode") return;
    const first = state.doc.lineAt(node.from);
    const marks = node.node.getChildren("CodeMark");
    const close = marks.length > 1 ? marks[marks.length - 1] : null;
    const bodyFrom = Math.min(first.to + 1, node.to);
    const bodyTo = close ? Math.max(bodyFrom, state.doc.lineAt(close.from).from - 1) : node.to;
    const info = node.node.getChild("CodeInfo");
    blocks.push({ from: node.from, to: node.to, bodyFrom, bodyTo,
      language: info ? state.doc.sliceString(info.from, info.to) : "text",
      code: state.doc.sliceString(bodyFrom, bodyTo) });
    return false;
  } });
  return blocks;
}

/** 非编辑状态显示整块代码，DOM 由文本节点构造，不执行笔记中的 HTML。 */
class CodeBlockWidget extends WidgetType {
  /** 保留源码位置快照，确保预览与编辑入口指向同一内容。 */
  constructor(private readonly block: CodeBlock, private readonly state: EditorState) { super(); }

  /** 语言异步装载需重新高亮；模式切换需移除或恢复编辑按钮，不能复用旧 DOM。 */
  eq(other: CodeBlockWidget): boolean {
    return this.block.from === other.block.from && this.block.to === other.block.to
      && this.block.code === other.block.code && this.block.language === other.block.language
      && !readingModeChanged(this.state, other.state)
      && syntaxTree(this.state) === syntaxTree(other.state);
  }

  /** 创建复制、换行和编辑入口，复制内容不包含语言标记与围栏。 */
  toDOM(view: EditorView): HTMLElement {
    const wrapper = document.createElement("section");
    wrapper.className = "qc-code-block";
    const header = wrapper.appendChild(document.createElement("div"));
    header.className = "qc-code-toolbar";
    const label = header.appendChild(document.createElement("span"));
    label.textContent = this.block.language;
    const pre = document.createElement("pre");
    const code = pre.appendChild(document.createElement("code"));
    let cursor = this.block.bodyFrom;
    highlightTree(syntaxTree(this.state), classHighlighter, (from, to, classes) => {
      if (from > cursor) code.append(document.createTextNode(this.state.doc.sliceString(cursor, from)));
      const span = code.appendChild(document.createElement("span"));
      span.className = classes;
      span.textContent = this.state.doc.sliceString(from, to);
      cursor = to;
    }, this.block.bodyFrom, this.block.bodyTo);
    code.append(document.createTextNode(this.state.doc.sliceString(cursor, this.block.bodyTo)));
    /** 工具栏独立处理操作，不让点击落到正文的源码入口。 */
    const addButton = (text: string, action: (button: HTMLButtonElement) => void): void => {
      const button = header.appendChild(document.createElement("button"));
      button.type = "button";
      button.textContent = text;
      button.addEventListener("click", () => action(button));
    };
    addButton("复制", (button) => {
      void navigator.clipboard.writeText(this.block.code).then(() => { button.textContent = "已复制"; })
        .catch(() => { button.textContent = "复制失败，请选中复制"; });
    });
    addButton("自动换行", (button) => {
      const wrap = pre.classList.toggle("is-wrapped");
      button.textContent = wrap ? "横向滚动" : "自动换行";
      button.setAttribute("aria-pressed", String(wrap));
      view.requestMeasure();
    });
    if (!this.state.facet(readingMode)) {
      addButton("编辑", () => {
        // 迟到的旧 DOM 点击也不能绕过当前阅读状态。
        if (view.state.facet(readingMode)) return;
        view.dispatch({ selection: { anchor: this.block.bodyFrom }, scrollIntoView: true });
        view.focus();
      });
    }
    wrapper.append(pre);
    // 点正文直接把光标放进代码，工具栏按钮由 focusSourceOnClick 放行。
    focusSourceOnClick(wrapper, view, () => this.block.bodyFrom);
    return wrapper;
  }

  /** 工具栏按钮自己处理点击；正文区域由 focusSourceOnClick 回到源码。 */
  ignoreEvent(): boolean { return true; }
}

/** 活动代码块保持源码可编辑，其余块整体预览；选择范围内的代码不替换。 */
function buildCodeDecorations(state: EditorState): DecorationSet {
  const ranges: Range<Decoration>[] = [];
  for (const block of collectCodeBlocks(state)) {
    const active = sourceSelectionTouches(state, block.from, block.to);
    if (!active) {
      ranges.push(Decoration.replace({ widget: new CodeBlockWidget(block, state), block: true }).range(block.from, block.to));
      continue;
    }
    const first = state.doc.lineAt(block.from).number;
    const last = state.doc.lineAt(block.to).number;
    for (let number = first; number <= last; number++) {
      ranges.push(Decoration.line({ class: "qc-code-line" }).range(state.doc.line(number).from));
    }
  }
  return Decoration.set(ranges, true);
}

/** 块替换需要直接状态装饰，不能使用影响视口布局的 ViewPlugin 装饰。 */
export const codeBlockPreview = StateField.define<DecorationSet>({
  create: buildCodeDecorations,
  /** 模式、内容与解析树变化均可能改变整块预览。 */
  update(value, transaction) {
    return transaction.docChanged || transaction.selection
      || readingModeChanged(transaction.startState, transaction.state)
      || syntaxTree(transaction.startState) !== syntaxTree(transaction.state) ? buildCodeDecorations(transaction.state) : value;
  },
  provide: (field) => EditorView.decorations.from(field),
});
