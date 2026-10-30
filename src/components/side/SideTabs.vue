<!--
  通用页签条：只负责"列出页签 + 回传选中项"。
  页签集合由调用方以数组给出，新增栏位只需加一条数据，不必在本组件里加分支。
-->
<script setup lang="ts">
defineProps<{
  /** 页签定义：id 用于比较与回传，label 是展示文案。 */
  tabs: { id: string; label: string }[];
  /** 当前选中页签的 id。 */
  active: string;
}>();

const emit = defineEmits<{ select: [id: string] }>();
</script>

<template>
  <div role="tablist" class="flex shrink-0 items-center gap-1 border-b border-hairline px-2 py-1.5">
    <button
      v-for="tab in tabs"
      :key="tab.id"
      type="button"
      role="tab"
      :aria-selected="tab.id === active"
      class="rounded-md px-2 py-1 text-[11px] transition"
      :class="tab.id === active ? 'bg-bg-paper font-medium text-ink' : 'text-ink-3 hover:text-ink-2'"
      @click="emit('select', tab.id)"
    >
      {{ tab.label }}
    </button>
  </div>
</template>
