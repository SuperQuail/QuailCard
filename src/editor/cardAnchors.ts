import type { Range } from "@codemirror/state";
import { Decoration, DecorationSet, EditorView, ViewPlugin, ViewUpdate, WidgetType } from "@codemirror/view";

/** 卡片锚点徽章：行内小标签，点击跳转卡片面板。 */
class AnchorChipWidget extends WidgetType {
  constructor(private readonly cardId: string) {
    super();
  }

  eq(other: AnchorChipWidget): boolean {
    return other.cardId === this.cardId;
  }

  toDOM(): HTMLElement {
    const span = document.createElement("span");
    span.className = "qc-anchor-chip";
    span.dataset.cardId = this.cardId;
    span.textContent = "已拆卡";
    return span;
  }

  ignoreEvent(): boolean {
    // 让点击事件穿透到编辑器，由 domEventHandlers 统一处理。
    return true;
  }
}

const anchorLineMark = Decoration.mark({ class: "qc-anchor-line" });

/** 扫描正文中的 `^qc-<id>` 锚点并生成装饰。 */
function buildAnchors(view: EditorView): DecorationSet {
  const ranges: Range<Decoration>[] = [];
  const text = view.state.doc.toString();
  const regex = /\^qc-([\w-]+)/g;
  let match: RegExpExecArray | null;
  while ((match = regex.exec(text)) !== null) {
    const from = match.index;
    const to = from + match[0].length;
    ranges.push(anchorLineMark.range(from, to));
    ranges.push(
      Decoration.widget({ widget: new AnchorChipWidget(match[1]), side: 1 }).range(from),
    );
  }
  return Decoration.set(ranges, true);
}

/** 卡片锚点装饰插件：句段高亮 + 可点击徽章。 */
export const cardAnchorPlugin = ViewPlugin.fromClass(
  class {
    decorations: DecorationSet;

    constructor(view: EditorView) {
      this.decorations = buildAnchors(view);
    }

    update(update: ViewUpdate): void {
      if (update.docChanged || update.viewportChanged) {
        this.decorations = buildAnchors(update.view);
      }
    }
  },
  { decorations: (plugin) => plugin.decorations },
);
