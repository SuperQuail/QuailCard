<script setup lang="ts">
import { computed, onBeforeUnmount, onMounted, ref, watch } from "vue";
import { ArrowUp, Plus, Settings, Square, X } from "@lucide/vue";
import type { NoteSummary } from "../../domain/types";
import type { AgentImage } from "../../domain/agent";
import { clipboardImages } from "../../services/agentImages";
import { looksLikeBilibili } from "../../services/videoLink";
import AgentImages from "./AgentImages.vue";
import VirtualScrollbar from "../VirtualScrollbar.vue";

/** 引用笔记列表与已选标签各一个滚动容器。 */
const noteListScroll = ref<HTMLElement | null>(null);
const selectedScroll = ref<HTMLElement | null>(null);

const props = defineProps<{
  draft: string; selectedPaths: string[];
  images?: AgentImage[]; readingImages?: boolean;
  notes: NoteSummary[]; ready: boolean; running: boolean; loading: boolean; error: string;
}>();
const emit = defineEmits<{
  send: [text: string]; stop: []; draft: [text: string]; scope: [paths: string[]];
  settings: [];
  pasteImages: [files: File[]]; removeImage: [index: number];
}>();
const scopeOpen = ref(false), noteQuery = ref("");
const textarea = ref<HTMLTextAreaElement | null>(null);
const canSend = computed(() => (!!props.draft.trim() || !!props.images?.length) && props.ready && !props.running && !props.loading && !props.readingImages);
/** 检测到 B 站链接时给出提示，实际解析与取字仍由后端工具完成。 */
const videoLinkHint = computed(() => looksLikeBilibili(props.draft));
const availableNotes = computed(() => props.notes.filter(note => note.path.toLowerCase().includes(noteQuery.value.toLowerCase())));
let resizeObserver: ResizeObserver | undefined;
let observedWidth = 0;

/** Ctrl+V 与系统粘贴均通过 paste 事件；含图片时保留随附纯文字及选区替换语义。 */
function paste(event: ClipboardEvent): void {
  const files = clipboardImages(event.clipboardData);
  if (!files.length) return;
  event.preventDefault();
  if (props.loading || props.running || props.readingImages) return;
  const text = event.clipboardData?.getData("text/plain");
  const element = textarea.value;
  if (text && element) emit("draft", props.draft.slice(0, element.selectionStart) + text + props.draft.slice(element.selectionEnd));
  emit("pasteImages", files);
}

/** 输入高度随草稿与窗口宽度变化，超过上限后仅在输入框内滚动。 */
function resizeInput(): void {
  const element = textarea.value;
  if (!element) return;
  element.style.height = "0px";
  element.style.height = `${Math.min(200, Math.max(48, element.scrollHeight))}px`;
  element.style.overflowY = element.scrollHeight > 200 ? "auto" : "hidden";
}
/** 输入 @ 时保留草稿，同时显示明确的资料范围选择。 */
function input(event: Event): void {
  const value = (event.target as HTMLTextAreaElement).value;
  emit("draft", value);
  if (value.endsWith("@")) scopeOpen.value = true;
  resizeInput();
}
/** 点击与键盘发送共用条件，避免空白或加载中的请求。 */
function send(): void { if (canSend.value) emit("send", props.draft); }
/** 中文候选确认和带修饰键的换行保留浏览器行为。 */
function keydown(event: KeyboardEvent): void {
  if (event.key !== "Enter" || event.isComposing || event.keyCode === 229 || event.shiftKey || event.ctrlKey || event.altKey || event.metaKey) return;
  event.preventDefault();
  send();
}
/** 空选择表示全库，单独移除标签与范围面板保持一致。 */
function toggleNote(path: string): void {
  emit("scope", props.selectedPaths.includes(path) ? props.selectedPaths.filter(item => item !== path) : [...props.selectedPaths, path]);
}
/** 父级清空或恢复草稿后同步输入框高度。 */
watch(() => props.draft, resizeInput, { flush: "post" });
/** 只响应宽度变化，避免高度调整触发观察器循环。 */
onMounted(() => {
  resizeInput();
  if (typeof ResizeObserver === "undefined") return;
  resizeObserver = new ResizeObserver(entries => {
    const width = entries[0]?.contentRect.width ?? 0;
    if (width !== observedWidth) { observedWidth = width; resizeInput(); }
  });
  if (textarea.value) resizeObserver.observe(textarea.value);
});
/** 组件退出时释放尺寸监听。 */
onBeforeUnmount(() => resizeObserver?.disconnect());
</script>

<template>
  <div class="space-y-3">
    <p v-if="error" class="px-3 text-[13px] text-danger" role="alert">{{ error }}</p>
    <div v-if="!ready" class="flex flex-wrap items-center justify-between gap-2 rounded-2xl bg-bg-side px-4 py-3 text-[13px]">
      <span>先配置模型，即可使用 Agent。</span>
      <button class="ghost-btn" @click="emit('settings')"><Settings :size="15" />模型设置</button>
    </div>
    <div class="rounded-[26px] border border-hairline bg-bg-paper p-3 shadow-[0_2px_12px_rgb(0_0_0/0.04)] transition-shadow focus-within:shadow-[0_2px_16px_rgb(0_0_0/0.08)]">
      <div v-if="scopeOpen" class="mb-3 rounded-2xl bg-bg-side p-3" @keydown.esc.stop="scopeOpen = false">
        <div class="mb-2 flex items-center gap-2">
          <input v-model="noteQuery" class="field-input min-w-0 flex-1" placeholder="搜索并选择笔记…" aria-label="搜索引用笔记" />
          <button class="icon-btn shrink-0" aria-label="关闭笔记选择" @click="scopeOpen = false"><X :size="16" /></button>
        </div>
        <div ref="noteListScroll" class="soft-scrollbar max-h-32 overflow-y-auto">
          <label v-for="note in availableNotes" :key="note.path" class="flex items-center gap-2 rounded-lg px-2 py-2 text-[13px] hover:bg-bg-hover">
            <input type="checkbox" :checked="selectedPaths.includes(note.path)" @change="toggleNote(note.path)" />
            <span class="min-w-0 break-all">{{ note.path }}</span>
          </label>
          <p v-if="!availableNotes.length" class="px-2 py-3 text-[13px] text-ink-2">{{ notes.length ? '没有匹配的笔记' : '知识库中暂无笔记' }}</p>
        </div>
        <VirtualScrollbar :target="noteListScroll" />
        <button v-if="selectedPaths.length" class="ghost-btn mt-2" @click="emit('scope', [])">切回全库</button>
      </div>
      <div v-if="selectedPaths.length" ref="selectedScroll" class="soft-scrollbar mb-2 flex max-h-20 flex-wrap gap-2 overflow-y-auto px-2">
        <span v-for="path in selectedPaths" :key="path" class="inline-flex max-w-full items-center gap-1 rounded-lg bg-bg-side py-1 pl-2 text-[12px] text-ink-2">
          <span class="truncate" :title="path">{{ path }}</span>
          <button class="grid size-6 shrink-0 place-items-center rounded hover:bg-bg-hover" :aria-label="`移除引用 ${path}`" @click="toggleNote(path)"><X :size="13" /></button>
        </span>
      </div>
      <VirtualScrollbar :target="selectedScroll" />
      <AgentImages :images="images ?? []" editable :disabled="running || readingImages" @remove="emit('removeImage', $event)" />
      <p v-if="readingImages" class="px-2 text-[12px] text-ink-2" role="status">正在读取图片…</p>
      <p v-if="videoLinkHint" class="px-2 text-[12px] text-ink-2" role="status">
        检测到 B 站链接：发送后 Agent 会先取字，需要成文时说一声即可。
      </p>
      <textarea ref="textarea" :value="draft" rows="1" class="block min-h-12 w-full resize-none border-0 bg-transparent px-2 py-2 text-[16px] leading-7 text-ink placeholder:text-ink-2 focus-visible:!outline-none"
        aria-label="给 Agent 发消息" placeholder="说说你想学习什么，或粘贴图片、@ 引用笔记" :disabled="loading" @input="input" @keydown="keydown" @paste="paste" />
      <div class="mt-2 flex items-center gap-2">
        <button class="flex h-9 min-w-0 max-w-[48%] items-center gap-1.5 rounded-full px-2 text-[12px] text-ink-2 hover:bg-bg-hover" :aria-expanded="scopeOpen" :title="selectedPaths.length ? '选择引用笔记' : '整个知识库 · 按需检索'" @click="scopeOpen = !scopeOpen">
          <Plus :size="19" class="shrink-0" /><span class="truncate">{{ selectedPaths.length ? `已选 ${selectedPaths.length} 篇` : '整个知识库' }}</span>
        </button>
        <span class="flex-1" />
        <button v-if="running" class="composer-action grid size-9 shrink-0 place-items-center rounded-full transition-colors" aria-label="停止生成" title="停止生成" @click="emit('stop')"><Square :size="14" fill="currentColor" /><span class="sr-only">停止</span></button>
        <button v-else class="composer-action grid size-9 shrink-0 place-items-center rounded-full transition-colors" aria-label="发送" title="发送" :disabled="!canSend" @click="send"><ArrowUp :size="20" :stroke-width="2.25" /><span class="sr-only">发送</span></button>
      </div>
    </div>
    <p class="px-2 text-center text-[11px] leading-5 text-ink-2"><span class="hidden sm:inline">Enter 发送 · Shift+Enter 换行 · </span>修改直接保存，可查看差异并撤销</p>
  </div>
</template>

<style scoped>
/* 与全局表单重置同处未分层样式，确保 currentColor 不被 color: inherit 覆盖。 */
.composer-action {
  background-color: var(--qc-ink);
  color: var(--qc-bg-paper);
}
.composer-action:hover:not(:disabled) {
  background-color: var(--qc-ink-2);
}
.composer-action:disabled {
  background-color: var(--qc-bg-active);
  color: var(--qc-ink-2);
  cursor: default;
}
</style>