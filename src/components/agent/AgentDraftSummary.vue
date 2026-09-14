<script setup lang="ts">
import { computed } from "vue";
import type { AgentMessage } from "../../domain/agent";
const props = defineProps<{ message: AgentMessage }>();
/** 草稿只提取文字，不创建复习 flow、勾选状态或采纳动作。 */
const cards = computed(() => {
  const raw = props.message.data?.cards;
  if (!Array.isArray(raw)) return [];
  return raw.flatMap((card: unknown) => {
    if (!card || typeof card !== "object" || !("fields" in card) || !card.fields || typeof card.fields !== "object") return [];
    const fields = card.fields as Record<string, unknown>;
    return [{ front: typeof fields.front === "string" ? fields.front : "", back: typeof fields.back === "string" ? fields.back : "" }];
  });
});
</script>
<template>
  <section class="draft-summary" aria-label="只读草稿摘要">
    <p>{{ cards.length }} 张卡片草稿 · 仅查看，不可采纳</p>
    <p v-if="message.content">{{ message.content }}</p>
    <details v-if="cards.length"><summary>查看草稿摘要</summary>
      <dl v-for="(card, index) in cards" :key="index"><dt>{{ card.front }}</dt><dd>{{ card.back }}</dd></dl>
    </details>
  </section>
</template>
<style scoped>
.draft-summary { padding: 10px; border: 1px solid var(--color-hairline); border-radius: 8px; font-size: 12px; }
p, dt, dd { white-space: pre-wrap; overflow-wrap: anywhere; } summary { cursor: pointer; } dl { margin-top: 8px; } dd { color: var(--color-ink-2); }
</style>
