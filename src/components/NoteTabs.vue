<script setup lang="ts">
import { FileText, X } from "@lucide/vue";
import { computed, nextTick, ref, watch } from "vue";
import VirtualScrollbar from "./VirtualScrollbar.vue";

const props = defineProps<{
  tabs: { path: string; title: string }[];
  activePath: string;
  busy?: boolean;
}>();
const emit = defineEmits<{ select: [path: string]; close: [path: string] }>();
const strip = ref<HTMLElement | null>(null);
/** 当前笔记被外部删除时，剩余标签仍保留一个键盘可达入口。 */
const focusPath = computed(() => props.tabs.some((tab) => tab.path === props.activePath) ? props.activePath : props.tabs[0]?.path);

/** 仅取得标签主按钮，关闭按钮不参与方向键游走。 */
function tabButtons(): HTMLButtonElement[] {
  return Array.from(strip.value?.querySelectorAll<HTMLButtonElement>('[role="tab"]') ?? []);
}

/** 激活标签时只滚动标签条本身，不滚动正文或整个工作区。 */
async function revealActive(): Promise<void> {
  await nextTick();
  const container = strip.value;
  const index = props.tabs.findIndex((tab) => tab.path === props.activePath);
  const item = tabButtons()[index]?.parentElement;
  if (!container || !item) return;
  const left = item.offsetLeft;
  const right = left + item.offsetWidth;
  if (left < container.scrollLeft) container.scrollLeft = left;
  else if (right > container.scrollLeft + container.clientWidth) container.scrollLeft = right - container.clientWidth;
}

/** 标签保持单一 Tab 入口；方向键、Home/End 切换，Delete 仅关闭标签不删文件。 */
function handleKey(event: KeyboardEvent, index: number): void {
  if (props.busy || event.isComposing) return;
  if (event.key === "Delete") {
    event.preventDefault(); emit("close", props.tabs[index].path); return;
  }
  const count = props.tabs.length;
  const destinations: Record<string, number> = { ArrowRight: (index + 1) % count, ArrowLeft: (index + count - 1) % count, Home: 0, End: count - 1 };
  const next = Object.prototype.hasOwnProperty.call(destinations, event.key) ? destinations[event.key] : undefined;
  if (next === undefined) return;
  event.preventDefault();
  tabButtons()[next]?.focus();
  emit("select", props.tabs[next].path);
}

/** 鼠标中键沿用桌面编辑器习惯，右键不误关闭标签。 */
function closeWithMiddle(event: MouseEvent, path: string): void {
  if (event.button !== 1 || props.busy) return;
  event.preventDefault(); emit("close", path);
}

/** 普通滚轮在标签溢出时横向移动，未溢出不截获滚动。 */
function scrollTabs(event: WheelEvent): void {
  const container = strip.value;
  if (!container || container.scrollWidth <= container.clientWidth || event.deltaX || !event.deltaY) return;
  event.preventDefault(); container.scrollLeft += event.deltaY;
}

/** 同名标签显示父目录，完整路径始终通过悬停和无障碍名称提供。 */
function parentHint(path: string, title: string): string {
  if (props.tabs.filter((tab) => tab.title === title).length < 2) return "";
  return path.split("/").slice(0, -1).join("/") || "根目录";
}

/** 标签顺序和当前项变化后保持当前标签可见，正文位置不受影响。 */
watch(() => [props.activePath, props.tabs.map((tab) => tab.path).join("\0")], revealActive, { immediate: true });
</script>

<template>
  <div class="note-tabs flex h-9 min-h-9 shrink-0 border-b border-hairline bg-bg-side">
    <div class="tab-viewport relative flex min-w-0 flex-1" @wheel="scrollTabs">
      <div ref="strip" role="tablist" aria-label="已打开的笔记" class="tab-strip flex min-w-0 flex-1 overflow-x-auto overflow-y-hidden">
        <div v-for="(tab, index) in tabs" :key="tab.path" class="note-tab group relative flex min-w-28 max-w-60 shrink-0 items-center border-r border-hairline" :class="{ 'is-active': tab.path === activePath }" @auxclick="closeWithMiddle($event, tab.path)">
          <button type="button" role="tab" :id="'note-tab-' + encodeURIComponent(tab.path)" aria-controls="note-editor-content" class="flex h-full min-w-0 flex-1 items-center gap-2 pl-3 pr-1 text-left text-[12px]" :aria-selected="tab.path === activePath" :aria-label="tab.path" :tabindex="tab.path === focusPath ? 0 : -1" :title="tab.path" :disabled="busy" @click="emit('select', tab.path)" @keydown="handleKey($event, index)">
            <FileText :size="14" :stroke-width="1.8" class="shrink-0 text-ink-3" aria-hidden="true" />
            <span class="truncate">{{ tab.title }}</span>
            <span v-if="parentHint(tab.path, tab.title)" class="max-w-20 truncate text-[10px] text-ink-3">{{ parentHint(tab.path, tab.title) }}</span>
          </button>
          <button type="button" class="tab-close mx-1 flex size-6 shrink-0 items-center justify-center rounded text-ink-3 hover:bg-bg-hover hover:text-ink" :aria-label="'关闭标签：' + tab.path" title="关闭标签（不删除笔记）" :tabindex="tab.path === focusPath ? 0 : -1" :disabled="busy" @click.stop="emit('close', tab.path)"><X :size="13" :stroke-width="1.8" aria-hidden="true" /></button>
        </div>
      </div>
      <VirtualScrollbar :target="strip" orientation="horizontal" />
    </div>
    <div class="tab-actions relative flex shrink-0 items-center px-3"><slot name="actions" /></div>
  </div>
</template>

<style scoped>
/* 原生水平滚动条在 Windows 上带两端箭头，还要吃掉一条高度；按 el-scrollbar 的 --hidden-default 藏掉原生条，
   位置交给浮层虚拟滚动条（VirtualScrollbar）。两个属性并存是为了兼容尚未支持 scrollbar-width 的旧 WebView2。 */
.tab-strip { scrollbar-width: none; }
.tab-strip::-webkit-scrollbar { display: none; }
.note-tab { color: var(--qc-ink-2); }
.note-tab.is-active { background: var(--qc-bg-paper); color: var(--qc-ink); box-shadow: inset 0 2px var(--qc-accent); }
.tab-close { opacity: 0; }
.note-tab:hover .tab-close, .note-tab:focus-within .tab-close, .note-tab.is-active .tab-close { opacity: 1; }
.note-tab button:focus-visible { outline: 1px solid var(--qc-accent); outline-offset: -2px; }
.note-tab button:disabled { cursor: default; opacity: 0.5; }
@media (hover: none) { .tab-close { opacity: 1; } }
</style>
