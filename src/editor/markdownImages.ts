import { syntaxTree } from "@codemirror/language";
import type { Extension, Range } from "@codemirror/state";
import { StateEffect } from "@codemirror/state";
import { Decoration, type DecorationSet, EditorView, ViewPlugin, type ViewUpdate, WidgetType } from "@codemirror/view";

/** 可展示的本地 Markdown 图片引用。 */
export interface MarkdownImageReference {
  from: number;
  to: number;
  alt: string;
  source: string;
}

type ImageState =
  | { status: "loading" }
  | { status: "loaded"; dataUrl: string }
  | { status: "error" };

const refreshImages = StateEffect.define<void>();

/** 排除网络、根路径、锚点和 Data URL，只把相对引用交给 Vault resolver。 */
export function isLocalRelativeImageSource(source: string): boolean {
  const value = source.trim();
  return Boolean(value)
    && !value.startsWith("/")
    && !value.startsWith("\\")
    && !value.startsWith("#")
    && !/^[a-z][a-z\d+.-]*:/i.test(value)
    && !/^[a-z]:[\\/]/i.test(value);
}

/** 从已确认的 Lezer Image 节点文本中提取 alt 与目标。 */
function parseImageNode(text: string): Pick<MarkdownImageReference, "alt" | "source"> | null {
  const match = text.match(/^!\[([^\]]*)\]\(\s*(?:<([^>\n]+)>|([^\s)\n]+))(?:\s+(?:"[^"]*"|'[^']*'|\([^)]*\)))?\s*\)$/);
  const source = match?.[2] ?? match?.[3] ?? "";
  return match && isLocalRelativeImageSource(source) ? { alt: match[1], source } : null;
}

/** 遍历 Markdown 语法树，只返回真正的本地 Image 节点。 */
export function collectMarkdownImages(state: Parameters<typeof syntaxTree>[0]): MarkdownImageReference[] {
  const images: MarkdownImageReference[] = [];
  syntaxTree(state).iterate({
    enter(node) {
      if (node.name !== "Image") {
        return;
      }
      const parsed = parseImageNode(state.doc.sliceString(node.from, node.to));
      if (parsed) {
        images.push({ from: node.from, to: node.to, ...parsed });
      }
    },
  });
  return images;
}

/** 图片预览 Widget：加载、成功和失败都在原位置给出明确状态。 */
class MarkdownImageWidget extends WidgetType {
  constructor(
    private readonly alt: string,
    private readonly source: string,
    private readonly imageState: ImageState,
  ) {
    super();
  }

  /** 相同载荷复用 DOM，减少滚动时闪烁。 */
  eq(other: MarkdownImageWidget): boolean {
    return other.alt === this.alt
      && other.source === this.source
      && JSON.stringify(other.imageState) === JSON.stringify(this.imageState);
  }

  /** 安全构造图片 DOM，不解析任意 HTML。 */
  toDOM(): HTMLElement {
    const wrapper = document.createElement("span");
    wrapper.className = `qc-image-preview is-${this.imageState.status}`;
    if (this.imageState.status === "loaded") {
      const image = document.createElement("img");
      image.src = this.imageState.dataUrl;
      image.alt = this.alt;
      image.title = this.source;
      wrapper.append(image);
    } else {
      wrapper.textContent = this.imageState.status === "loading" ? "正在载入图片…" : `图片无法显示：${this.alt || this.source}`;
    }
    return wrapper;
  }
}

/** 创建绑定单篇笔记的预览插件，销毁后忽略所有迟到响应。 */
export function markdownImagePreview(
  notePath: string,
  resolver: (notePath: string, source: string) => Promise<string>,
): Extension {
  return ViewPlugin.fromClass(class {
    decorations: DecorationSet = Decoration.none;
    private readonly states = new Map<string, ImageState>();
    private active = true;

    /** 初始化可见图片并发起缺失资源读取。 */
    constructor(private readonly view: EditorView) {
      this.decorations = this.buildDecorations();
    }

    /** 文档、光标、视口或图片状态变化时重建预览。 */
    update(update: ViewUpdate): void {
      if (update.docChanged || update.selectionSet || update.viewportChanged || update.transactions.some((transaction) => transaction.effects.some((effect) => effect.is(refreshImages)))) {
        this.decorations = this.buildDecorations();
      }
    }

    /** 停止迟到的异步结果触发旧编辑器刷新。 */
    destroy(): void {
      this.active = false;
    }

    /** 返回缓存状态，并在首次遇到来源时启动解析。 */
    private stateFor(source: string): ImageState {
      const existing = this.states.get(source);
      if (existing) {
        return existing;
      }
      const loading: ImageState = { status: "loading" };
      this.states.set(source, loading);
      void resolver(notePath, source).then((dataUrl) => {
        this.finishLoad(source, { status: "loaded", dataUrl });
      }).catch(() => {
        this.finishLoad(source, { status: "error" });
      });
      return loading;
    }

    /** 仅在插件仍存活时接收图片结果并请求一次装饰刷新。 */
    private finishLoad(source: string, state: ImageState): void {
      if (!this.active) {
        return;
      }
      this.states.set(source, state);
      this.view.dispatch({ effects: refreshImages.of(undefined) });
    }

    /** 为可见范围内且不在光标行的图片构造替换装饰。 */
    private buildDecorations(): DecorationSet {
      const cursorLine = this.view.state.doc.lineAt(this.view.state.selection.main.head).number;
      const ranges: Range<Decoration>[] = [];
      for (const image of collectMarkdownImages(this.view.state)) {
        const visible = this.view.visibleRanges.some((range) => image.from <= range.to && image.to >= range.from);
        if (!visible || this.view.state.doc.lineAt(image.from).number === cursorLine) {
          continue;
        }
        const widget = new MarkdownImageWidget(image.alt, image.source, this.stateFor(image.source));
        ranges.push(Decoration.replace({ widget }).range(image.from, image.to));
      }
      return Decoration.set(ranges, true);
    }
  }, { decorations: (plugin) => plugin.decorations });
}
