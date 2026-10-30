import { syntaxTree } from "@codemirror/language";
import type { Extension, Range } from "@codemirror/state";
import { StateEffect } from "@codemirror/state";
import { Decoration, type DecorationSet, EditorView, ViewPlugin, type ViewUpdate, WidgetType } from "@codemirror/view";
import { focusSourceOnClick } from "./sourceOnClick";
import { readingModeChanged, sourceSelectionTouches } from "./readingMode";

/** 可展示的本地 Markdown 图片引用。 */
export interface MarkdownImageReference {
  from: number;
  to: number;
  alt: string;
  source: string;
}

/** 图片预览的可选交互钩子：编辑器只上报引用与事件，菜单形态由壳层决定。 */
export interface MarkdownImageOptions {
  /** 右键已加载图片时回调；不提供则保留原生菜单，编辑模式不吞掉用户默认操作。 */
  onContextMenu?: (image: MarkdownImageReference, event: MouseEvent) => void;
}

/** 插件构造期创建的右键派发函数，身份稳定，可安全交给会被复用的 widget。 */
type ContextMenuDispatcher = (image: MarkdownImageReference, event: MouseEvent) => void;

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
  /** 图片载荷与源码位置一起保存，异步刷新后仍能正确回到源码。 */
  constructor(
    private readonly from: number,
    private readonly to: number,
    private readonly alt: string,
    private readonly source: string,
    private readonly imageState: ImageState,
    private readonly dispatchContextMenu: ContextMenuDispatcher | undefined,
  ) {
    super();
  }

  /**
   * 相同载荷且位置未变时复用 DOM，减少滚动时闪烁；位置变化必须重建，否则点击会跳错位置。
   * 派发函数不参与比较：它在插件构造期创建且身份稳定，复用 DOM 后仍指向当前回调。
   */
  eq(other: MarkdownImageWidget): boolean {
    return other.from === this.from
      && other.to === this.to
      && other.alt === this.alt
      && other.source === this.source
      && JSON.stringify(other.imageState) === JSON.stringify(this.imageState);
  }

  /** 安全构造图片 DOM，不解析任意 HTML。 */
  toDOM(view: EditorView): HTMLElement {
    const wrapper = document.createElement("span");
    wrapper.className = `qc-image-preview is-${this.imageState.status}`;
    if (this.imageState.status === "loaded") {
      const image = document.createElement("img");
      // 图片解码前后高度差很大，而解码不会产生 DOM 变动；不通知重新测绘，
      // CodeMirror 的高度表会一直停留在占位高度，之后所有点击的落点都会整体偏移。
      image.addEventListener("load", () => view.requestMeasure());
      image.src = this.imageState.dataUrl;
      image.alt = this.alt;
      image.title = this.source;
      wrapper.append(image);
      // 只有解码成功的图片才有可复制/定位的文件；加载中与失败态预览不响应菜单。
      if (this.dispatchContextMenu) {
        wrapper.addEventListener("contextmenu", (event) => {
          event.preventDefault();
          event.stopPropagation();
          this.dispatchContextMenu?.({
            from: this.from, to: this.to, alt: this.alt, source: this.source,
          }, event);
        });
      }
    } else {
      wrapper.textContent = this.imageState.status === "loading" ? "正在载入图片…" : `图片无法显示：${this.alt || this.source}`;
    }
    // 点图片回到源码，便于直接改路径或删除。
    focusSourceOnClick(wrapper, view, () => this.from);
    return wrapper;
  }
}

/** 创建绑定单篇笔记的预览插件，销毁后忽略所有迟到响应。 */
export function markdownImagePreview(
  notePath: string,
  resolver: (notePath: string, source: string) => Promise<string>,
  options: MarkdownImageOptions = {},
): Extension {
  return ViewPlugin.fromClass(class {
    decorations: DecorationSet = Decoration.none;
    private readonly states = new Map<string, ImageState>();
    /**
     * 插件实例内唯一的右键派发函数。
     *
     * 原因：widget.eq 只比较可序列化载荷，函数身份无法比较；若每个 widget 各自
     * 捕获闭包，DOM 复用后会继续用旧 options。这里每个插件只建一次，widget 复用安全。
     */
    private readonly dispatchContextMenu: ContextMenuDispatcher | undefined = options.onContextMenu
      ? (image, event) => { options.onContextMenu?.(image, event); }
      : undefined;
    private active = true;

    /** 初始化可见图片并发起缺失资源读取。 */
    constructor(private readonly view: EditorView) {
      this.decorations = this.buildDecorations();
    }

    /** 文档、光标、视口或图片状态变化时重建预览。 */
    update(update: ViewUpdate): void {
      if (update.docChanged || update.selectionSet || update.viewportChanged
        || readingModeChanged(update.startState, update.state) || update.transactions.some((transaction) => transaction.effects.some((effect) => effect.is(refreshImages)))) {
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

    /** 为可见范围内且未被选区碰到的图片构造替换装饰。 */
    private buildDecorations(): DecorationSet {
      const ranges: Range<Decoration>[] = [];
      for (const image of collectMarkdownImages(this.view.state)) {
        const visible = this.view.visibleRanges.some((range) => image.from <= range.to && image.to >= range.from);
        // 与表格、代码块一致：只有选区碰到图片本身才回到源码，同一行其他位置不影响预览。
        const active = sourceSelectionTouches(this.view.state, image.from, image.to);
        if (!visible || active) {
          continue;
        }
        const widget = new MarkdownImageWidget(image.from, image.to, image.alt, image.source,
          this.stateFor(image.source), this.dispatchContextMenu);
        ranges.push(Decoration.replace({ widget }).range(image.from, image.to));
      }
      return Decoration.set(ranges, true);
    }
  }, { decorations: (plugin) => plugin.decorations });
}
