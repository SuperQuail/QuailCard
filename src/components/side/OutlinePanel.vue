<script setup lang="ts">
import type { OutlineItem } from "../../domain/outline";

/**
 * 大纲面板：只负责展示标题与回传跳转偏移。
 * 解析、当前小节计算和编辑器滚动都在调用方，组件不持有正文也不直接操作编辑器。
 */
const props = defineProps<{
  items: OutlineItem[];
  /** 编辑器视口顶部所属标题的起始偏移；null/负数表示没有可高亮的标题。 */
  activeFrom?: number | null;
}>();

const emit = defineEmits<{ jump: [from: number] }>();

/** 缩进随层级递增，层级差异只用缩进表达，字号统一 12px 以保证长文档可扫读。 */
function indentFor(level: number): string {
  return `${8 + Math.max(0, level - 1) * 12}px`;
}

/** 当前小节由 activeFrom 精确匹配决定，避免把相邻标题也误高亮。 */
function isActive(item: OutlineItem): boolean {
  return props.activeFrom != null && props.activeFrom >= 0 && item.from === props.activeFrom;
}
</script>

<template>
  <ul class="soft-scrollbar h-full overflow-y-auto px-3 py-2" aria-label="大纲">
    <li v-for="item in items" :key="item.from">
      <button
        type="button"
        class="block w-full truncate rounded-md py-1 pr-2 text-left text-[12px] leading-5 text-ink-2 transition hover:bg-bg-hover"
        :class="{ 'bg-bg-active/60 text-ink': isActive(item), 'font-semibold text-ink': item.level === 1 }"
        :style="{ paddingLeft: indentFor(item.level) }"
        :aria-current="isActive(item) ? 'true' : undefined"
        :title="item.text"
        @click="emit('jump', item.from)"
      >
        {{ item.text }}
      </button>
    </li>
    <li v-if="items.length === 0" class="px-2 py-4 text-[11px] text-ink-3">没有标题</li>
  </ul>
</template>
