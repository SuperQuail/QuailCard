<script setup lang="ts">
import { FileText, Sparkles, X } from "@lucide/vue";
import { defaultKeymap, history, historyKeymap } from "@codemirror/commands";
import { markdown, markdownLanguage } from "@codemirror/lang-markdown";
import { Compartment } from "@codemirror/state";
import { EditorView, highlightActiveLine, keymap } from "@codemirror/view";
import { onBeforeUnmount, onMounted, ref, watch } from "vue";
import { cardAnchorPlugin } from "../editor/cardAnchors";
import { hideMarkersPlugin } from "../editor/hideMarkers";
import { imageTransferExtension } from "../editor/imageTransfer";
import { markdownImagePreview } from "../editor/markdownImages";
import { markdownTheme } from "../editor/markdownTheme";
import { resolveAttachmentDataUrl } from "../services/attachmentService";

const props = defineProps<{
  notePath: string;
  content: string;
  dark: boolean;
}>();

const emit = defineEmits<{
  "card-click": [cardId: string];
  "create-card": [selection: string];
  "save-content": [notePath: string, content: string];
}>();

/** 划词浮条：选区文本与屏幕坐标。 */
interface SelectionBar {
  text: string;
  x: number;
  y: number;
}

const container = ref<HTMLElement | null>(null);
const selectionBar = ref<SelectionBar | null>(null);
const editorError = ref("");
const themeCompartment = new Compartment();
const imageCompartment = new Compartment();
let view: EditorView | null = null;
let saveTimer: number | undefined;
let pendingSaveNotePath = "";
let syncingDocument = false;

/** 创建 CodeMirror 编辑器实例。 */
function createEditor(): void {
  if (!container.value || view) {
    return;
  }
  view = new EditorView({
    parent: container.value,
    doc: props.content,
    extensions: [
      history(),
      highlightActiveLine(),
      keymap.of([...defaultKeymap, ...historyKeymap]),
      markdown({ base: markdownLanguage }),
      themeCompartment.of(markdownTheme(props.dark)),
      imageCompartment.of(markdownImagePreview(props.notePath, resolveAttachmentDataUrl)),
      imageTransferExtension(() => props.notePath, (message) => { editorError.value = message; }),
      cardAnchorPlugin,
      hideMarkersPlugin,
      EditorView.updateListener.of((update) => {
        if (update.docChanged && !syncingDocument) {
          scheduleSave();
        }
      }),
      EditorView.domEventHandlers({
        /** 点击卡片锚点徽章时跳转卡片面板。 */
        click(event) {
          const target = event.target as HTMLElement | null;
          const chip = target?.closest(".qc-anchor-chip") as HTMLElement | null;
          if (chip?.dataset.cardId) {
            emit("card-click", chip.dataset.cardId);
          }
        },
      }),
      EditorView.lineWrapping,
    ],
  });
}

/** 防抖自动保存。 */
function scheduleSave(): void {
  if (saveTimer) {
    window.clearTimeout(saveTimer);
  }
  pendingSaveNotePath = props.notePath;
  saveTimer = window.setTimeout(() => {
    flushSave();
  }, 600);
}

/** 立即提交待保存正文，并保留产生变更时的笔记身份。 */
function flushSave(): void {
  if (!saveTimer || !view || !pendingSaveNotePath) {
    return;
  }
  window.clearTimeout(saveTimer);
  saveTimer = undefined;
  emit("save-content", pendingSaveNotePath, view.state.doc.toString());
  pendingSaveNotePath = "";
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
  const selection = window.getSelection();
  if (!selection || selection.isCollapsed || selection.rangeCount === 0) {
    return null;
  }
  const range = selection.getRangeAt(0);
  const rect = range.getBoundingClientRect();
  const text = selection.toString().trim();
  if (!text) {
    return null;
  }
  return {
    text,
    x: Math.min(Math.max(rect.left + rect.width / 2, 120), window.innerWidth - 120),
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
    emit("create-card", selectionBar.value.text);
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

/** 主题切换：重新配置主题扩展。 */
watch(
  () => props.dark,
  (dark) => {
    if (view) {
      view.dispatch({ effects: themeCompartment.reconfigure(markdownTheme(dark)) });
    }
  },
);

/** 笔记切换必定换文档；同笔记外部变化仅在失焦时同步。 */
watch(
  () => [props.notePath, props.content] as const,
  ([notePath, content], [previousNotePath]) => {
    if (!view) {
      return;
    }
    const noteChanged = notePath !== previousNotePath;
    if (noteChanged) {
      flushSave();
      editorError.value = "";
    }
    if ((noteChanged || !view.hasFocus) && content !== view.state.doc.toString()) {
      syncingDocument = true;
      view.dispatch({ changes: { from: 0, to: view.state.doc.length, insert: content } });
      syncingDocument = false;
    }
    if (noteChanged) {
      view.dispatch({ effects: imageCompartment.reconfigure(markdownImagePreview(notePath, resolveAttachmentDataUrl)) });
    }
  },
);

onMounted(() => {
  createEditor();
  window.addEventListener("mouseup", handleSelectionEnd);
  window.addEventListener("mousedown", handleGlobalEvent);
  window.addEventListener("keydown", handleGlobalEvent);
});

onBeforeUnmount(() => {
  window.removeEventListener("mouseup", handleSelectionEnd);
  window.removeEventListener("mousedown", handleGlobalEvent);
  window.removeEventListener("keydown", handleGlobalEvent);
  flushSave();
  view?.destroy();
  view = null;
});
</script>

<template>
  <div class="flex h-full flex-col">
    <!-- 页面标题栏：路径信息 -->
    <header class="sticky top-0 z-20 flex h-[47px] items-center gap-1.5 border-b border-hairline bg-bg-paper/92 px-4 text-[11px] text-ink-3 backdrop-blur-sm">
      <FileText :size="13" class="shrink-0" />
      <span class="truncate">{{ notePath }}</span>
      <span v-if="editorError" class="ml-auto truncate text-danger" :title="editorError">{{ editorError }}</span>
      <span v-else class="flex shrink-0 items-center gap-1.5">
        <span class="size-1.5 rounded-full bg-success" />
        已保存
      </span>
    </header>

    <!-- CodeMirror 编辑器：单一实例，行内即时渲染 -->
    <div ref="container" class="soft-scrollbar min-h-0 flex-1" />

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
  </div>
</template>
