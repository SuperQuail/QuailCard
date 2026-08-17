<script setup lang="ts">
import { Brain, FilePlus2, FileText, Layers, PenLine, Search, Settings, Sparkles, SunMoon } from "@lucide/vue";
import { onMounted, ref } from "vue";
import { useAppState } from "../services/appState";
import type { NoteSummary } from "../domain/types";

const props = defineProps<{
  notes: NoteSummary[];
}>();

const emit = defineEmits<{
  close: [];
  "select-note": [path: string];
  "select-card": [notePath: string, cardId: string];
  "run-action": [actionId: string];
}>();

const store = useAppState();

/** 可导航的扁平结果项。 */
interface ResultItem {
  kind: "action" | "note" | "card";
  id: string;
  label: string;
  hint: string;
  actionId?: string;
  notePath?: string;
  cardId?: string;
}

/** 全局命令列表。 */
const actions: Array<{ id: string; label: string; hint: string }> = [
  { id: "new-note", label: "新建笔记", hint: "Ctrl+N" },
  { id: "capture", label: "快速捕获", hint: "" },
  { id: "today-review", label: "开始今日复习", hint: "" },
  { id: "toggle-theme", label: "切换深色 / 浅色模式", hint: "" },
  { id: "toggle-panel", label: "显示 / 隐藏卡片面板", hint: "" },
  { id: "settings", label: "打开设置", hint: "" },
];

const query = ref("");
const selectedIndex = ref(0);
const results = ref<ResultItem[]>([]);
const searching = ref(false);

/** 返回命令项使用的图标组件。 */
function resolveActionIcon(actionId?: string) {
  if (actionId === "new-note") {
    return FilePlus2;
  }
  if (actionId === "capture") {
    return PenLine;
  }
  if (actionId === "today-review") {
    return Brain;
  }
  if (actionId === "toggle-theme") {
    return SunMoon;
  }
  if (actionId === "toggle-panel") {
    return Layers;
  }
  if (actionId === "settings") {
    return Settings;
  }
  return Sparkles;
}

/** 关键词变化时更新结果：命令本地过滤，笔记/卡片走后端 FTS。 */
async function refreshResults(): Promise<void> {
  const keyword = query.value.trim().toLocaleLowerCase();
  const items: ResultItem[] = actions
    .filter((action) => !keyword || action.label.toLocaleLowerCase().includes(keyword))
    .map((action) => ({ kind: "action" as const, id: action.id, label: action.label, hint: action.hint, actionId: action.id }));
  results.value = items;
  selectedIndex.value = 0;
  if (!keyword) {
    for (const note of props.notes.slice(0, 5)) {
      items.push({ kind: "note", id: note.path, label: note.title, hint: note.path, notePath: note.path });
    }
    return;
  }
  searching.value = true;
  try {
    const found = await store.search(keyword);
    for (const note of found.notes) {
      items.push({ kind: "note", id: note.path, label: note.title, hint: note.path, notePath: note.path });
    }
    for (const card of found.cards) {
      items.push({ kind: "card", id: card.cardId, label: card.front, hint: card.notePath, notePath: card.notePath, cardId: card.cardId });
    }
  } finally {
    searching.value = false;
  }
  results.value = items;
}

/** 执行选中项的动作。 */
function runItem(item: ResultItem): void {
  if (item.kind === "action" && item.actionId) {
    emit("run-action", item.actionId);
  } else if (item.kind === "note" && item.notePath) {
    emit("select-note", item.notePath);
  } else if (item.kind === "card" && item.notePath && item.cardId) {
    emit("select-card", item.notePath, item.cardId);
  }
  emit("close");
}

/** 键盘导航：上下选择、回车执行。 */
function handleKeydown(event: KeyboardEvent): void {
  if (event.key === "ArrowDown") {
    event.preventDefault();
    selectedIndex.value = Math.min(selectedIndex.value + 1, results.value.length - 1);
  } else if (event.key === "ArrowUp") {
    event.preventDefault();
    selectedIndex.value = Math.max(selectedIndex.value - 1, 0);
  } else if (event.key === "Enter") {
    event.preventDefault();
    const item = results.value[selectedIndex.value];
    if (item) {
      runItem(item);
    }
  }
}

onMounted(() => {
  void refreshResults();
  document.getElementById("palette-input")?.focus();
});
</script>

<template>
  <div class="overlay-backdrop flex items-start justify-center pt-28" @click.self="emit('close')">
    <div class="modal-panel w-full max-w-[560px] overflow-hidden !p-0" @keydown="handleKeydown">
      <div class="flex items-center gap-2.5 border-b border-hairline px-4">
        <Search :size="16" class="shrink-0 text-ink-3" />
        <input
          id="palette-input"
          v-model="query"
          class="palette-input h-12 min-w-0 flex-1 bg-transparent text-[14px] outline-none"
          placeholder="搜索笔记、卡片，或输入命令…"
          @keydown.esc="emit('close')"
          @input="void refreshResults()"
        />
        <span v-if="searching" class="shrink-0 text-[11px] text-ink-3">搜索中…</span>
        <kbd v-else class="kbd shrink-0">Esc</kbd>
      </div>

      <ul class="soft-scrollbar max-h-[46vh] overflow-y-auto p-1.5">
        <li v-if="results.length === 0" class="px-3 py-6 text-center text-[12px] text-ink-3">没有匹配的结果</li>
        <li
          v-for="(item, itemIndex) in results"
          :key="`${item.kind}-${item.id}`"
          class="flex cursor-pointer items-center gap-2.5 rounded-md px-3 py-2 transition"
          :class="selectedIndex === itemIndex ? 'bg-bg-active' : ''"
          @mouseenter="selectedIndex = itemIndex"
          @click="runItem(item)"
        >
          <component :is="resolveActionIcon(item.kind === 'action' ? item.actionId : undefined)" :size="15" class="shrink-0 text-ink-3" v-if="item.kind === 'action'" />
          <FileText v-else-if="item.kind === 'note'" :size="15" class="shrink-0 text-ink-3" />
          <Layers v-else :size="15" class="shrink-0 text-ink-3" />
          <div class="min-w-0 flex-1">
            <p class="truncate text-[13px]">{{ item.label }}</p>
            <p v-if="item.kind !== 'action'" class="truncate text-[11px] text-ink-3">{{ item.hint }}</p>
          </div>
          <span v-if="item.kind === 'action'" class="shrink-0 text-[10px] text-ink-3">{{ item.hint }}</span>
          <span v-else-if="item.kind === 'note'" class="shrink-0 rounded-full bg-marker px-1.5 text-[9px] font-semibold text-accent-strong">笔记</span>
          <span v-else class="shrink-0 rounded-full bg-marker px-1.5 text-[9px] font-semibold text-accent-strong">卡片</span>
        </li>
      </ul>

      <footer class="flex items-center gap-3 border-t border-hairline px-4 py-2 text-[10px] text-ink-3">
        <span class="flex items-center gap-1"><kbd class="kbd">↑</kbd><kbd class="kbd">↓</kbd> 选择</span>
        <span class="flex items-center gap-1"><kbd class="kbd">Enter</kbd> 打开</span>
      </footer>
    </div>
  </div>
</template>

<style scoped>
.palette-input:focus-visible {
  outline: none;
}
</style>
