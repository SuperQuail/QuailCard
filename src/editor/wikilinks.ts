import { syntaxTree } from "@codemirror/language";
import type { EditorState, Extension, Range } from "@codemirror/state";
import {
  Decoration,
  type DecorationSet,
  EditorView,
  ViewPlugin,
  type ViewUpdate,
  WidgetType,
} from "@codemirror/view";
import {
  WIKILINK,
  splitWikiTarget,
  wikiLinkDisplayText,
  wikiLinkSpan,
  type WikiLinkSpan,
} from "../markdown/wikilink";
import { readingMode, readingModeChanged } from "./readingMode";

/** 打开双链时回传的原始目标：保留 #标题，由宿主决定跳转到哪篇笔记的哪一行。 */
export type WikiLinkOpenHandler = (target: string) => void;

/** 解析判定由宿主注入：装饰层不依赖 store，也不需要知道当前笔记路径。 */
export type WikiLinkResolver = (target: string) => boolean;

/** 双链扩展的注入点；两个回调都在编辑时动态读取，不缓存解析结果。 */
export interface WikiLinkOptions {
  onOpen: WikiLinkOpenHandler;
  isResolved: WikiLinkResolver;
}

/** 阅读态把 [[目标|显示]] 折叠成显示名或目标名，点击即可打开。 */
class WikiLinkWidget extends WidgetType {
  constructor(
    private readonly span: WikiLinkSpan,
    private readonly resolved: boolean,
    private readonly onOpen: WikiLinkOpenHandler,
  ) {
    super();
  }

  /** 位置、目标或解析状态变化都必须重建，否则点击会打开旧目标。 */
  eq(other: WikiLinkWidget): boolean {
    return other.span.from === this.span.from
      && other.span.to === this.span.to
      && other.span.target === this.span.target
      && other.span.label === this.span.label
      && other.span.embed === this.span.embed
      && other.resolved === this.resolved;
  }

  /** 阅读态只显示文字，不把 Markdown 原文带进阅读视图。 */
  toDOM(): HTMLElement {
    const span = document.createElement("span");
    span.className = `qc-wikilink-preview${this.resolved ? " is-resolved" : " is-unresolved"}${this.span.embed ? " is-embed" : ""}`;
    span.textContent = wikiLinkDisplayText(this.span);
    span.title = this.span.embed ? `内嵌：${this.span.target}` : this.span.target;
    span.addEventListener("click", (event) => {
      event.preventDefault();
      this.onOpen(this.span.target);
    });
    return span;
  }

  /** 点击已由 widget 处理，编辑器不再把它当光标定位事件。 */
  ignoreEvent(): boolean {
    return true;
  }
}

/** 编辑态用颜色区分可解析与不可解析，同时保留原文（含 [[ ]]）。 */
function markClass(resolved: boolean): string {
  return resolved ? "qc-wikilink is-resolved" : "qc-wikilink is-unresolved";
}

/** 扫描语法树生成装饰；阅读态整段替换，编辑态只做样式标记。 */
function buildDecorations(state: EditorState, options: WikiLinkOptions): DecorationSet {
  const ranges: Range<Decoration>[] = [];
  const content = state.doc.toString();
  const reading = state.facet(readingMode);
  syntaxTree(state).iterate({
    enter(node) {
      if (node.name !== WIKILINK) return;
      const span = wikiLinkSpan(node.node, content);
      // 解析只针对笔记部分：# 之后是笔记内标题锚点，不参与笔记存在性判断。
      const resolved = options.isResolved(splitWikiTarget(span.target).note);
      if (reading) {
        ranges.push(Decoration.replace({
          widget: new WikiLinkWidget(span, resolved, options.onOpen),
          block: false,
        }).range(node.from, node.to));
        return false;
      }
      ranges.push(Decoration.mark({
        class: markClass(resolved),
        attributes: { title: resolved ? "Ctrl/Cmd+点击打开双链" : "目标笔记不存在" },
      }).range(node.from, node.to));
      return false;
    },
  });
  return Decoration.set(ranges, true);
}

/** 在事件落点向上找最近的双链节点；找不到返回 null。 */
function wikiLinkAt(event: MouseEvent, view: EditorView): WikiLinkSpan | null {
  let position: number;
  try {
    position = view.posAtDOM(event.target as Node, 0);
  } catch {
    return null;
  }
  let node: ReturnType<typeof syntaxTree>["topNode"] | null = syntaxTree(view.state).resolveInner(position, 1);
  while (node && node.name !== WIKILINK) node = node.parent;
  if (!node) return null;
  return wikiLinkSpan(node, view.state.doc.toString());
}

/**
 * 双链装饰扩展。
 * - 编辑态：显示原文，已解析/未解析用不同样式；Ctrl/Cmd+点击回传目标。
 * - 阅读态：只显示 | 后面的显示名或目标名，普通点击回传目标。
 * 回调由宿主注入，组件/编辑器不直接做导航。
 */
export function wikilinkExtension(options: WikiLinkOptions): Extension {
  const plugin = ViewPlugin.fromClass(
    class {
      decorations: DecorationSet;

      /** 初次挂载时按当前阅读状态构建，不依赖后续事务。 */
      constructor(view: EditorView) {
        this.decorations = buildDecorations(view.state, options);
      }

      /** 文档、视口或阅读模式变化都会改变装饰范围。 */
      update(update: ViewUpdate): void {
        if (update.docChanged || update.viewportChanged
          || readingModeChanged(update.startState, update.state)
          || syntaxTree(update.startState) !== syntaxTree(update.state)) {
          this.decorations = buildDecorations(update.state, options);
        }
      }
    },
    {
      decorations: (instance) => instance.decorations,
      eventHandlers: {
        mousedown(event, view) {
          // 只在编辑态响应 Ctrl/Cmd+点击；阅读态的点击由 widget 自己处理。
          if (view.state.facet(readingMode)) return false;
          if (event.button !== 0 || (!event.ctrlKey && !event.metaKey)) return false;
          const span = wikiLinkAt(event as MouseEvent, view);
          if (!span) return false;
          event.preventDefault();
          options.onOpen(span.target);
          return true;
        },
      },
    },
  );

  const theme = EditorView.theme({
    ".qc-wikilink": {
      cursor: "text",
      textDecoration: "underline",
      textDecorationThickness: "1px",
      textUnderlineOffset: "2px",
    },
    ".qc-wikilink.is-resolved": {
      color: "var(--qc-accent)",
      textDecorationColor: "var(--qc-accent)",
    },
    ".qc-wikilink.is-unresolved": {
      color: "var(--qc-ink-3)",
      textDecorationStyle: "dashed",
      textDecorationColor: "var(--qc-ink-3)",
    },
    ".qc-wikilink-preview": {
      color: "var(--qc-accent)",
      cursor: "pointer",
      borderBottom: "1px solid color-mix(in srgb, var(--qc-accent) 45%, transparent)",
    },
    ".qc-wikilink-preview.is-unresolved": {
      color: "var(--qc-ink-3)",
      borderBottomStyle: "dashed",
      borderBottomColor: "var(--qc-ink-3)",
    },
    ".qc-wikilink-preview.is-embed": {
      fontStyle: "italic",
    },
  });

  return [plugin, theme];
}
