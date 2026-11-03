<script setup lang="ts">
import { nextTick, ref, watch } from "vue";
import TypingCell from "./TypingCell.vue";
import type { TypingAttempt, TypingPage, TypingRecord } from "../../domain/typingDrill";
import type { TypingPhase } from "../../services/typingSession";

/**
 * 一页练习网格：未轮到的格子只占位，已完成的格子保留记录。
 * 记忆遍期间整页加 is-memory，由样式把本页已有记录一起模糊。
 */
const props = defineProps<{
  page: TypingPage;
  phase: TypingPhase;
  doneCount: number;
  attempt: TypingAttempt | null;
  records: Record<string, TypingRecord>;
  memoryMode: boolean;
}>();

const root = ref<HTMLElement | null>(null);

/** 尚未轮到的格子不出内容，避免提前看到后面的单词。 */
function cellState(index: number): "pending" | "active" | "done" {
  if (index < props.doneCount) return "done";
  if (index === props.doneCount && props.phase === "drill") return "active";
  return "pending";
}

/** 当前格变化后滚到视野中央；测试环境没有 scrollIntoView 时静默跳过。 */
watch(
  () => props.attempt?.itemId,
  async (itemId) => {
    if (!itemId || !root.value) return;
    await nextTick();
    const cell = root.value.querySelector<HTMLElement>(`[data-item-id="${itemId}"]`);
    cell?.scrollIntoView?.({ block: "center" });
  },
);
</script>

<template>
  <div ref="root" class="typing-board" :class="{ 'is-memory': memoryMode }" aria-label="练习页">
    <TypingCell
      v-for="(item, index) in page.items"
      :key="item.id"
      :item="item"
      :index="index + 1"
      :state="cellState(index)"
      :attempt="index === doneCount ? attempt : null"
      :record="records[item.id] ?? null"
    />
  </div>
</template>
