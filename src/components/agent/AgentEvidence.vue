<script setup lang="ts">
import type { GoalEvidence } from "../../domain/agent";
defineProps<{ evidence: GoalEvidence[]; criteria?: string[] }>();
</script>
<template>
  <details v-if="evidence.length" class="agent-evidence">
    <summary>查看证据来源 {{ evidence.length }}</summary>
    <dl v-for="(item, index) in evidence" :key="index">
      <dt>验收条件 · criterionIndex</dt><dd>{{ item.criterionIndex }}<template v-if="criteria?.[item.criterionIndex]"> · {{ criteria[item.criterionIndex] }}</template></dd>
      <dt>目标版本 · goalRevision</dt><dd>{{ item.goalRevision }}</dd>
      <dt>来源版本 · sourceVersion</dt><dd>{{ item.sourceVersion }}</dd>
      <dt>收据引用 · receiptRef</dt><dd>{{ item.receiptRef }}</dd>
    </dl>
  </details>
</template>
<style scoped>
.agent-evidence { margin-top: 6px; font-size: 12px; overflow-wrap: anywhere; }
summary { cursor: pointer; color: var(--color-ink-2); }
dl { display: grid; grid-template-columns: auto minmax(0, 1fr); gap: 4px 10px; margin-top: 8px; }
dt { color: var(--color-ink-3); } dd { white-space: pre-wrap; }
</style>
