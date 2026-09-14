import type { Range } from "@codemirror/state";
import { Decoration, DecorationSet, EditorView, ViewPlugin, ViewUpdate, WidgetType } from "@codemirror/view";
import { syntaxTree } from "@codemirror/language";
import type { NoteCard } from "../domain/types";
import { isCardSourceCurrent } from "../domain/cardSource";

/** 打开卡片的动作：徽章点击后交给宿主组件，编辑器自己不做导航。 */
export type OpenCardHandler = (cardId: string) => void;

/** 卡片锚点徽章：行内小标签，点击打开卡片面板。 */
class AnchorChipWidget extends WidgetType {
  constructor(private readonly cardId: string, private readonly onOpenCard: OpenCardHandler) {
    super();
  }

  eq(other: AnchorChipWidget): boolean {
    return other.cardId === this.cardId;
  }

  /** 徽章不在编辑器事件链上：widget 默认吞掉事件，点击只能由徽章自己处理。 */
  toDOM(): HTMLElement {
    const span = document.createElement("span");
    span.className = "qc-anchor-chip";
    span.textContent = "已拆卡";
    span.title = "打开卡片";
    span.addEventListener("click", () => this.onOpenCard(this.cardId));
    return span;
  }

  /** 点击已由徽章处理，编辑器不再把它当作光标定位或选区的一部分。 */
  ignoreEvent(): boolean {
    return true;
  }
}

const anchorLineMark = Decoration.mark({ class: "qc-anchor-line" });

/** 扫描正文中的 `^qc-<id>` 锚点并生成装饰。 */
function buildAnchors(view: EditorView, onOpenCard: OpenCardHandler): DecorationSet {
  const ranges: Range<Decoration>[] = [];
  const text = view.state.doc.toString();
  const regex = /\^qc-([\w-]+)/g;
  let match: RegExpExecArray | null;
  while ((match = regex.exec(text)) !== null) {
    const from = match.index;
    const to = from + match[0].length;
    ranges.push(anchorLineMark.range(from, to));
    ranges.push(
      Decoration.widget({ widget: new AnchorChipWidget(match[1], onOpenCard), side: 1 }).range(from),
    );
  }
  return Decoration.set(ranges, true);
}

/** 卡片锚点装饰插件：句段高亮 + 可点击徽章。 */
export function cardAnchorPlugin(onOpenCard: OpenCardHandler) {
  return ViewPlugin.fromClass(
    class {
      decorations: DecorationSet;

      constructor(view: EditorView) {
        this.decorations = buildAnchors(view, onOpenCard);
      }

      update(update: ViewUpdate): void {
        if (update.docChanged || update.viewportChanged) {
          this.decorations = buildAnchors(update.view, onOpenCard);
        }
      }
    },
    { decorations: (plugin) => plugin.decorations },
  );
}

/** 新来源徽章只装饰文档，不把标记写进 Markdown 或代码内容。 */
export function cardSourceBadges(cards: NoteCard[], onOpenCard: OpenCardHandler) {
  return ViewPlugin.fromClass(class {
    decorations: DecorationSet;
    /** 初始化当前笔记卡片的来源装饰。 */
    constructor(view: EditorView) { this.decorations = this.build(view); }
    /** 原文编辑后重新验证来源，失效时隐藏而不是猜测匹配。 */
    update(update: ViewUpdate): void {
      if (update.docChanged || update.viewportChanged) this.decorations = this.build(update.view);
    }
    /** 代码块由独立预览管理，代码内不插入徽章。 */
    build(view: EditorView): DecorationSet {
      const document = view.state.doc.toString();
      const ranges: Range<Decoration>[] = [];
      for (const card of cards) {
        if (!card.source || !isCardSourceCurrent(document, card.source)) continue;
        let node = syntaxTree(view.state).resolveInner(card.source.from, 1);
        let inCode = false;
        while (node.parent) { if (node.name === "FencedCode" || node.name === "CodeBlock") inCode = true; node = node.parent; }
        if (!inCode) ranges.push(Decoration.widget({ widget: new AnchorChipWidget(card.id, onOpenCard), side: 1 }).range(card.source.to));
      }
      return Decoration.set(ranges, true);
    }
  }, { decorations: (plugin) => plugin.decorations });
}
