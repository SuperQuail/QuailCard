<script setup lang="ts">
import { computed } from "vue";
import { X } from "@lucide/vue";
import type { AgentChange } from "../../domain/agent";
import { agentDiff } from "../../domain/agentDiff";
import { documentKindOf } from "../../domain/documentKind";
import { diffWordGroups } from "../../domain/wordGroup";

const props = defineProps<{ change: AgentChange; busy: boolean }>();
const emit = defineEmits<{ close: []; undo: [id: string]; note: [path: string] }>();
const lines = computed(() => agentDiff(props.change.before, props.change.after));
/** 单词组优先用页级列表：整份 JSON 的文本 diff 噪声太大，改没改哪几个词看不出来。 */
const wordGroupChanges = computed(() => (documentKindOf(props.change.path) === "wordGroup" ? diffWordGroups(props.change.before, props.change.after) : null));
const labels: Record<string, string> = { applied: "已应用", undone: "已撤销", notApplied: "未应用", conflict: "存在后续变化", pending: "等待核对", undoing: "等待恢复核对" };
const pageLabels: Record<string, string> = { added: "新增", removed: "删除", changed: "修改" };
const pageTone: Record<string, string> = { added: "text-success", removed: "text-danger", changed: "text-ink-2" };
const title = computed(() => (wordGroupChanges.value ? "Agent 单词组差异" : "Agent 笔记差异"));
</script>
<template>
  <div class="fixed inset-0 z-50 flex items-center justify-center bg-ink/30 p-6" @click.self="emit('close')">
    <section class="flex max-h-[85vh] w-full max-w-4xl flex-col rounded-xl border border-hairline bg-bg-paper shadow-xl" role="dialog" aria-modal="true" :aria-label="title" @keydown.esc.stop="emit('close')">
      <header class="flex items-center gap-3 border-b border-hairline p-4"><p class="min-w-0 flex-1 truncate text-[13px]">{{ change.path }} · {{ labels[change.state] ?? change.state }}</p><button class="icon-btn" aria-label="关闭差异" @click="emit('close')"><X :size="16" /></button></header>
      <div v-if="wordGroupChanges" class="min-h-0 overflow-auto px-4 py-3 text-[12px] leading-6">
        <p v-if="!wordGroupChanges.length" class="text-[12px] text-ink-3">没有可展示的页级改动（内容可能只变了格式）。</p>
        <ul v-else class="space-y-2">
          <li v-for="(page, index) in wordGroupChanges" :key="index" class="rounded-lg border border-hairline px-3 py-2">
            <p class="flex items-center gap-2">
              <span class="shrink-0 text-[11px]" :class="pageTone[page.kind]">{{ pageLabels[page.kind] }}</span>
              <span class="min-w-0 break-all font-medium">{{ page.word || "（未填单词）" }}</span>
            </p>
            <p v-if="page.kind === 'changed'" class="mt-1 space-y-0.5 text-[11px] text-ink-3">
              <span v-if="page.before?.meaning.trim() !== page.after?.meaning.trim()" class="block break-all"><span class="text-danger">− {{ page.before?.meaning || "（空）" }}</span></span>
              <span v-if="page.before?.meaning.trim() !== page.after?.meaning.trim()" class="block break-all"><span class="text-success">+ {{ page.after?.meaning || "（空）" }}</span></span>
              <span v-if="page.before?.phonetic.trim() !== page.after?.phonetic.trim()" class="block break-all">音标：{{ page.before?.phonetic || "（空）" }} → {{ page.after?.phonetic || "（空）" }}</span>
              <span v-if="page.before?.example.trim() !== page.after?.example.trim()" class="block break-all">例句已更新</span>
            </p>
            <p v-else class="mt-1 break-all text-[11px] text-ink-3">{{ (page.kind === "removed" ? page.before?.meaning : page.after?.meaning) || "（暂无释义）" }}</p>
          </li>
        </ul>
      </div>
      <div v-else class="min-h-0 overflow-auto py-3 font-mono text-[12px] leading-6">
        <pre v-for="(line, index) in lines" :key="index" class="whitespace-pre-wrap break-all px-4" :class="{ 'bg-success/10 text-success': line.kind === 'add', 'bg-danger/10 text-danger': line.kind === 'remove' }">{{ line.kind === 'add' ? '+' : line.kind === 'remove' ? '−' : ' ' }} {{ line.text }}</pre>
      </div>
      <footer class="flex flex-wrap items-center gap-3 border-t border-hairline p-4 text-[11px] text-ink-3"><span class="flex-1">撤销前会检查后续编辑；新建{{ wordGroupChanges ? "单词组" : "笔记" }}撤销后保留回收副本。</span><button class="ghost-btn" @click="emit('note', change.path)">{{ wordGroupChanges ? "打开单词组" : "打开笔记" }}</button><button class="primary-btn" :disabled="busy || change.state !== 'applied'" @click="emit('undo', change.id)">撤销此改动</button></footer>
    </section>
  </div>
</template>
