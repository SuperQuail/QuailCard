<script setup lang="ts">
import { Sparkles, X } from "@lucide/vue";
import NoteToolbar from "./NoteToolbar.vue";
import NoteTabs from "./NoteTabs.vue";
import ImageContextMenu from "./ImageContextMenu.vue";
import { useEditorTabCache } from "../composables/useEditorTabCache";
import { useImageMenu } from "../composables/useImageMenu";
import { defaultKeymap, history, historyKeymap } from "@codemirror/commands";
import { markdown, markdownLanguage } from "@codemirror/lang-markdown";
import { Compartment, Transaction } from "@codemirror/state";
import { languages } from "@codemirror/language-data";
import { syntaxHighlighting } from "@codemirror/language";
import { classHighlighter } from "@lezer/highlight";
import { captureCardSource } from "../domain/cardSource";
import { clampImageMenuPosition } from "../domain/imageMenu";
import type { CardSelection, NoteCard } from "../domain/types";
import { codeBlockPreview } from "../editor/codeBlocks";
import { codeBlockTheme } from "../editor/codeBlockTheme";
import { markdownMathPreview } from "../editor/markdownMath";
import { markdownTablePreview, markdownTableTheme } from "../editor/markdownTables";
import { markdownExtensions } from "../markdown/parser";
import { frontmatterExtension } from "../markdown/frontmatterView";
import { EditorView, keymap } from "@codemirror/view";
import { externalNoteUpdate, useEditorReadingMode } from "../composables/useEditorReadingMode";
import { onBeforeUnmount, onMounted, ref, watch } from "vue";
import { cardAnchorPlugin, cardSourceBadges } from "../editor/cardAnchors";
import { hideMarkersPlugin } from "../editor/hideMarkers";
import { remeasureOnContentResize } from "../editor/contentResize";
import { markdownListGlyphs } from "../editor/markdownLists";
import { markdownHorizontalRules } from "../editor/markdownRules";
import { imageTransferExtension } from "../editor/imageTransfer";
import { markdownImagePreview, type MarkdownImageReference } from "../editor/markdownImages";
import { markdownTheme } from "../editor/markdownTheme";
import { resolveAttachmentDataUrl } from "../services/attachmentService";
import { noteSearchExtension } from "../editor/noteSearch";
import { wikilinkExtension } from "../editor/wikilinks";
import { wikilinkCompletion } from "../editor/wikilinkCompletion";
import { splitWikiTarget } from "../markdown/wikilink";
import { resolveLink } from "../domain/links";
import type { NoteSummary } from "../domain/types";
import VirtualScrollbar from "./VirtualScrollbar.vue";

/** CodeMirror 真正的滚动容器：容器 div 自己不滚动，滚动几何只能问 .cm-scroller。 */
const editorScroll = ref<HTMLElement | null>(null);

const props = defineProps<{
  notePath: string;
  content: string;
  dark: boolean;
  panelOpen: boolean;
  readOnly?: boolean;
  cards?: NoteCard[];
  tabs?: { path: string; title: string }[];
  tabsBusy?: boolean;
  /** 全部笔记摘要：双链的解析判定与补全候选都来自这里，组件自身不查 store。 */
  linkTargets?: NoteSummary[];
}>();

const emit = defineEmits<{
  "card-click": [cardId: string];
  "create-card": [selection: CardSelection];
  "toggle-panel": [];
  "select-tab": [path: string];
  "close-tab": [path: string];
  "save-content": [notePath: string, content: string];
  /** 点击 [[双链]] 时上抛目标；解析与打开由壳层决定。 */
  "open-link": [target: string];
  /** 视口顶部所在的字符偏移（节流上报），供大纲换算当前高亮项。 */
  "viewport-change": [offset: number];
  /** 图片菜单动作结果提示：编辑器只上抛文案，提示组件由壳层统一渲染。 */
  toast: [message: string];
}>();

/** 划词浮条：选区文本与屏幕坐标。 */
interface SelectionBar {
  selection: CardSelection;
  x: number;
  y: number;
}

const container = ref<HTMLElement | null>(null);
const selectionBar = ref<SelectionBar | null>(null);
const editorError = ref("");
const themeCompartment = new Compartment();
const imageCompartment = new Compartment();
const cardsCompartment = new Compartment();
let view: EditorView | null = null;
const readingMode = useEditorReadingMode(() => view, () => Boolean(props.readOnly));
const { reading } = readingMode;
// 解构保留 ref：script setup 只解包顶层 ref，嵌套在对象里的 ref 在模板中不会自动解包。
const {
  context: imageMenu, busy: imageMenuBusy, open: openImageMenu, close: closeImageMenu,
  copyImage, saveImage, revealImage,
} = useImageMenu({ showToast: (message) => emit("toast", message) });
const tabCache = useEditorTabCache();
let syncingDocument = false;
let editorGeneration = 0;

/** 判断 [[目标]] 是否指向已存在的笔记；#标题 不参与解析。 */
function isLinkResolved(target: string): boolean {
  return resolveLink(splitWikiTarget(target).note, props.notePath, props.linkTargets ?? []) !== null;
}

/**
 * 视口顶部偏移的节流上报：滚动与编辑都会触发，但大纲只需要稳定结果。
 * 同一个定时器内的多次事件合并成一次 emit，避免每帧重算大纲高亮。
 */
let viewportTimer: number | null = null;
/** 排队一次视口上报；已有待发事件时直接复用。 */
function scheduleViewportReport(current: EditorView): void {
  if (viewportTimer !== null) return;
  viewportTimer = window.setTimeout(() => {
    viewportTimer = null;
    if (view === current) {
      emit("viewport-change", current.lineBlockAtHeight(0).from);
    }
  }, 150);
}

/** 滚动到正文内某个字符偏移并把光标放过去（大纲跳转）。 */
function revealPosition(from: number): void {
  if (!view) return;
  const pos = Math.max(0, Math.min(from, view.state.doc.length));
  view.dispatch({ selection: { anchor: pos }, effects: EditorView.scrollIntoView(pos, { y: "start" }) });
  view.focus();
}

/** 滚动到 1 起始的第 line 行（反链跳转）；行号越界时夹到首末行。 */
function revealLine(line: number): void {
  if (!view) return;
  const doc = view.state.doc;
  revealPosition(doc.line(Math.max(1, Math.min(line, doc.lines))).from);
}

/** 读取 CodeMirror 的真实选区，失焦后仍能用于智能拆卡。 */
function getSelection(): CardSelection | null {
  if (!view || reading.value || props.readOnly || view.state.selection.main.empty) return null;
  const { from, to } = view.state.selection.main;
  return { notePath: props.notePath, source: captureCardSource(view.state.doc.toString(), from, to) };
}

defineExpose({ getSelection, revealPosition, revealLine });

/** 把图片右键坐标钳制进视口后交给菜单用例；菜单如何展示由壳层决定。 */
function handleImageContextMenu(image: MarkdownImageReference, event: MouseEvent): void {
  openImageMenu({
    notePath: props.notePath,
    source: image.source,
    ...clampImageMenuPosition(event.clientX, event.clientY, { width: window.innerWidth, height: window.innerHeight }),
  });
}

/** 创建 CodeMirror 编辑器实例。 */
function createEditor(): void {
  if (!container.value || view) return;
  const saved = tabCache.take(props.notePath, props.content);
  readingMode.restore(saved?.reading);
  // 每次激活换代，旧图片请求即使切回同一路径也不能继续导入或报告错误。
  const generation = ++editorGeneration;
  const images = [markdownImagePreview(props.notePath, resolveAttachmentDataUrl, { onContextMenu: handleImageContextMenu }),
    imageTransferExtension(() => generation === editorGeneration ? props.notePath : "",
      (message) => { if (generation === editorGeneration) editorError.value = message; })];
  const state = saved?.state?.update({ effects: imageCompartment.reconfigure([]) }).state
    .update({ effects: imageCompartment.reconfigure(images) }).state;
  view = new EditorView({
    parent: container.value,
    state,
    scrollTo: saved?.scroll?.effect,
    doc: props.content,
    extensions: [
      history(),
      readingMode.extensions(),
      keymap.of([...defaultKeymap, ...historyKeymap]),
      // 编辑器与显示侧共用同一份自定义语法登记表（公式、frontmatter、双链）。
      markdown({ base: markdownLanguage, extensions: [...markdownExtensions], codeLanguages: languages }),
      syntaxHighlighting(classHighlighter),
      codeBlockPreview,
      codeBlockTheme,
      markdownTablePreview,
      markdownTableTheme,
      markdownMathPreview,
      themeCompartment.of(markdownTheme(props.dark)),
      imageCompartment.of(images),
      cardAnchorPlugin((cardId) => emit("card-click", cardId)),
      cardsCompartment.of(cardSourceBadges(props.cards ?? [], (cardId) => emit("card-click", cardId))),
      hideMarkersPlugin,
      markdownListGlyphs,
      markdownHorizontalRules,
      remeasureOnContentResize,
      noteSearchExtension(),
      // 双链的解析状态与补全候选都由 props 提供，装饰层不依赖 store。
      wikilinkExtension({ onOpen: (target) => emit("open-link", target), isResolved: isLinkResolved }),
      wikilinkCompletion(() => props.linkTargets ?? []),
      frontmatterExtension(),
      EditorView.updateListener.of((update) => {
        if (update.view !== view) return;
        readingMode.trackUpdate(update);
        if (update.viewportChanged || update.docChanged) {
          scheduleViewportReport(update.view);
        }
        if (update.docChanged && !syncingDocument) {
          emit("save-content", props.notePath, update.state.doc.toString());
        }
      }),
      EditorView.lineWrapping,
    ],
  });
  // 视图建好才有 .cm-scroller；重建编辑器时这里会重新取一次，浮层不会指向旧节点。
  editorScroll.value = container.value?.querySelector<HTMLElement>(".cm-scroller") ?? null;
  if (saved?.state) { refreshConfiguration(); readingMode.refresh(); }
  tabCache.restoreScroll(view, saved);
}

/** 判断选区是否发生在编辑器内部。 */
function selectionInsideEditor(): boolean {
  const selection = window.getSelection();
  if (!selection || selection.isCollapsed || selection.rangeCount === 0 || !container.value) {
    return false;
  }
  const range = selection.getRangeAt(0);
  return container.value.contains(range.commonAncestorContainer);
}

/** 读取当前选区的文本与位置。 */
function captureSelection(): SelectionBar | null {
  if (!view || reading.value || props.readOnly) return null;
  const { from, to, empty } = view.state.selection.main;
  if (empty) return null;
  const source = captureCardSource(view.state.doc.toString(), from, to);
  const rect = view.coordsAtPos(from);
  if (!source.excerpt.trim() || !rect) return null;
  return {
    selection: { notePath: props.notePath, source },
    x: Math.min(Math.max((rect.left + rect.right) / 2, 120), window.innerWidth - 120),
    y: Math.max(rect.top - 12, 64),
  };
}

/** 鼠标抬起时检测划词，显示拆卡浮条。 */
function handleSelectionEnd(): void {
  if (selectionInsideEditor()) {
    selectionBar.value = captureSelection();
  } else if (!window.getSelection()?.toString().trim()) {
    selectionBar.value = null;
  }
}

/** 提交划词拆卡并清除选区。 */
function createCardFromSelection(): void {
  if (selectionBar.value) {
    emit("create-card", selectionBar.value.selection);
  }
  selectionBar.value = null;
  window.getSelection()?.removeAllRanges();
}

/** 全局事件：Esc 或点击空白处收起浮条。 */
function handleGlobalEvent(event: Event): void {
  if (event instanceof KeyboardEvent && event.key === "Escape") {
    selectionBar.value = null;
    return;
  }
  if (event instanceof MouseEvent && selectionBar.value) {
    window.setTimeout(() => {
      if (!window.getSelection()?.toString().trim()) {
        selectionBar.value = null;
      }
    }, 0);
  }
}

/** 恢复缓存也必须使用当前主题与卡片，不能沿用非活跃时的配置。 */
function refreshConfiguration(): void {
  view?.dispatch({ effects: [
    themeCompartment.reconfigure(markdownTheme(props.dark)),
    cardsCompartment.reconfigure(cardSourceBadges(props.cards ?? [], (cardId) => emit("card-click", cardId))),
  ] });
}
watch(() => [props.dark, props.cards], refreshConfiguration);

/** 草稿由保存服务保护；接受已确认的外部内容，同时防止同步事务再次触发保存。 */
watch(
  () => [props.notePath, props.content] as const,
  ([notePath, content], [previousNotePath]) => {
    if (!view) return;
    if (notePath !== previousNotePath) {
      editorError.value = "";
      selectionBar.value = null;
      tabCache.save(previousNotePath, view, readingMode.snapshot());
      if (props.tabs) tabCache.retain(props.tabs.map((tab) => tab.path));
      view.destroy();
      view = null;
      createEditor();
      return;
    }
    if (content !== view.state.doc.toString()) {
      syncingDocument = true;
      try {
        view.dispatch({ changes: { from: 0, to: view.state.doc.length, insert: content },
          annotations: [externalNoteUpdate.of(true), Transaction.addToHistory.of(false)] });
      } finally { syncingDocument = false; }
    }
  },
);

/** 改名时冻结输入，防止清空保存队列后又产生旧路径写入。 */
watch(() => props.readOnly, () => readingMode.refresh());

/** 阅读时不沿用隐藏的编辑选区，避免误拆卡。 */
watch(reading, () => { selectionBar.value = null; });

/** 关闭、删除及改名立即淘汰非活跃缓存；活跃项在离开时再次核验。 */
watch(() => props.tabs?.map((tab) => tab.path), (paths) => { if (paths) tabCache.retain(paths); });

onMounted(() => {
  createEditor();
  window.addEventListener("mouseup", handleSelectionEnd);
  window.addEventListener("mousedown", handleGlobalEvent);
  window.addEventListener("keydown", handleGlobalEvent);
});

onBeforeUnmount(() => {
  if (viewportTimer !== null) { window.clearTimeout(viewportTimer); viewportTimer = null; }
  window.removeEventListener("mouseup", handleSelectionEnd);
  window.removeEventListener("mousedown", handleGlobalEvent);
  window.removeEventListener("keydown", handleGlobalEvent);
  view?.destroy();
  view = null;
  tabCache.clear();
  editorGeneration++;
});
</script>

<template>
  <div class="editor-pane relative flex h-full min-h-0 flex-col overflow-hidden" :class="{ 'has-tabs': tabs?.length }">
    <!-- 标签和操作共用一排，不重复保留空工具栏。 -->
    <NoteTabs v-if="tabs?.length" :tabs="tabs" :active-path="notePath" :busy="tabsBusy" @select="emit('select-tab', $event)" @close="emit('close-tab', $event)">
      <template #actions>
        <NoteToolbar inline :note-path="notePath" :panel-open="panelOpen" :editor-error="editorError" :reading="reading" @toggle-reading="readingMode.toggle" @toggle-panel="emit('toggle-panel')" />
      </template>
    </NoteTabs>
    <NoteToolbar v-else :note-path="notePath" :panel-open="panelOpen" :editor-error="editorError" :reading="reading" @toggle-reading="readingMode.toggle" @toggle-panel="emit('toggle-panel')" />

    <!-- CodeMirror 编辑器：单一实例，行内即时渲染。三层盒子同尺寸，浮层与编辑器同壳即同坐标系。 -->
    <div class="relative flex min-h-0 flex-1">
    <div ref="container" class="min-h-0 min-w-0 flex-1" :id="tabs?.length ? 'note-editor-content' : undefined" :role="tabs?.length ? 'tabpanel' : undefined" :aria-labelledby="tabs?.length ? 'note-tab-' + encodeURIComponent(notePath) : undefined" />

    <VirtualScrollbar :target="editorScroll" />
    <!-- 宽表格、长代码行与图片会让编辑器横向溢出，横向同样用浮层表达，不留原生条。 -->
    <VirtualScrollbar :target="editorScroll" orientation="horizontal" />
    </div>

    <!-- 划词浮条 -->
    <div
      v-if="selectionBar"
      class="selection-bar"
      :style="{ left: `${selectionBar.x}px`, top: `${selectionBar.y}px`, transform: 'translate(-50%, -100%)' }"
    >
      <button type="button" class="ghost-btn !text-accent-strong" @click="createCardFromSelection">
        <Sparkles :size="13" />拆成卡片
      </button>
      <button type="button" class="icon-btn" title="关闭" @click="selectionBar = null">
        <X :size="13" />
      </button>
    </div>

    <!-- 图片右键菜单自带 Teleport，放在编辑器壳层内即可，不参与布局。 -->
    <ImageContextMenu
      v-if="imageMenu"
      :request="imageMenu"
      :busy="imageMenuBusy"
      @copy="void copyImage()"
      @save="void saveImage()"
      @reveal="void revealImage()"
      @close="closeImageMenu"
    />
  </div>
</template>

<style scoped>
.editor-pane { container-type: inline-size; }
/* 无标签时工具栏图标绝对定位在右上角，查找面板要给它们让出宽度，否则关闭按钮会被压住。 */
.editor-pane:not(.has-tabs) :deep(.cm-search) { padding-right: 88px; }
/* 内容区较窄时为按钮留右侧空隙，不额外增加顶部空行，也不遮挡文字。 */
@container (max-width: 1360px) {
  .editor-pane:not(.has-tabs) :deep(.cm-content) { padding-right: 88px; }
}
</style>
