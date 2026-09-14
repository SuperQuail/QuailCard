<script setup lang="ts">
import { computed } from "vue";
import { X } from "@lucide/vue";
import type { AgentChange } from "../../domain/agent";
import { agentDiff } from "../../domain/agentDiff";
const props = defineProps<{ change: AgentChange; busy: boolean }>();
const emit = defineEmits<{ close: []; undo: [id: string]; note: [path: string] }>();
const lines = computed(() => agentDiff(props.change.before, props.change.after));
const labels: Record<string, string> = { applied: "已应用", undone: "已撤销", notApplied: "未应用", conflict: "存在后续变化", pending: "等待核对", undoing: "等待恢复核对" };
</script>
<template>
  <div class="fixed inset-0 z-50 flex items-center justify-center bg-ink/30 p-6" @click.self="emit('close')">
    <section class="flex max-h-[85vh] w-full max-w-4xl flex-col rounded-xl border border-hairline bg-bg-paper shadow-xl" role="dialog" aria-modal="true" aria-label="Agent 笔记差异" @keydown.esc.stop="emit('close')">
      <header class="flex items-center gap-3 border-b border-hairline p-4"><p class="min-w-0 flex-1 truncate text-[13px]">{{ change.path }} · {{ labels[change.state] ?? change.state }}</p><button class="icon-btn" aria-label="关闭差异" @click="emit('close')"><X :size="16" /></button></header>
      <div class="min-h-0 overflow-auto py-3 font-mono text-[12px] leading-6">
        <pre v-for="(line, index) in lines" :key="index" class="whitespace-pre-wrap break-all px-4" :class="{ 'bg-success/10 text-success': line.kind === 'add', 'bg-danger/10 text-danger': line.kind === 'remove' }">{{ line.kind === 'add' ? '+' : line.kind === 'remove' ? '−' : ' ' }} {{ line.text }}</pre>
      </div>
      <footer class="flex flex-wrap items-center gap-3 border-t border-hairline p-4 text-[11px] text-ink-3"><span class="flex-1">撤销前会检查后续编辑；新建笔记撤销后保留回收副本。</span><button class="ghost-btn" @click="emit('note', change.path)">打开笔记</button><button class="primary-btn" :disabled="busy || change.state !== 'applied'" @click="emit('undo', change.id)">撤销此改动</button></footer>
    </section>
  </div>
</template>
