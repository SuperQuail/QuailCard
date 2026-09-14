<script setup lang="ts">
import { computed, ref } from "vue";
import { ChevronRight, Zap } from "@lucide/vue";
import { agentToolCallRows } from "../../domain/agentToolCalls";
import type { AgentMessage } from "../../domain/agent";
import VirtualScrollbar from "../VirtualScrollbar.vue";

/** 参数与结果两块各自一个滚动容器。 */
const argsScroll = ref<HTMLElement | null>(null);
const outputScroll = ref<HTMLElement | null>(null);
const props = defineProps<{ message: AgentMessage }>();
const rows = computed(() => agentToolCallRows(props.message));
const open = ref<string | null>(null);
/** 参数与结果只在展开时格式化，长正文不进入折叠行。 */
function pretty(raw: string | null): string {
  if (!raw) return "";
  try {
    return JSON.stringify(JSON.parse(raw), null, 2);
  } catch {
    return raw;
  }
}
</script>
<template>
  <div v-if="rows.length" class="space-y-1">
    <div v-for="row in rows" :key="row.id" class="rounded-lg border border-hairline" :data-state="row.state">
      <button type="button" class="flex w-full min-w-0 items-center gap-2 px-2 py-1 text-left text-[12px] transition-colors hover:bg-bg-side"
        :aria-expanded="open === row.id" @click="open = open === row.id ? null : row.id">
        <Zap :size="13" class="shrink-0 text-ink-3" />
        <span class="shrink-0 font-mono text-ink-2">{{ row.name }}</span>
        <span class="min-w-0 flex-1 truncate" :class="row.state === 'error' ? 'text-danger' : 'text-ink-3'">{{ row.summary }}</span>
        <ChevronRight :size="14" class="shrink-0 text-ink-3 transition-transform" :class="{ 'rotate-90': open === row.id }" />
      </button>
      <div v-if="open === row.id" class="space-y-2 border-t border-hairline px-3 py-2 text-[12px]">
        <div>
          <p class="mb-1 text-ink-3">参数</p>
          <pre ref="argsScroll" class="soft-scrollbar max-h-64 overflow-auto whitespace-pre-wrap break-words rounded bg-bg-side px-2 py-1 font-mono text-[11px] leading-5">{{ pretty(row.arguments) }}</pre>
          <VirtualScrollbar :target="argsScroll" />
        </div>
        <div v-if="row.output">
          <p class="mb-1 text-ink-3">结果</p>
          <pre ref="outputScroll" class="soft-scrollbar max-h-64 overflow-auto whitespace-pre-wrap break-words rounded bg-bg-side px-2 py-1 font-mono text-[11px] leading-5">{{ pretty(row.output) }}</pre>
          <VirtualScrollbar :target="outputScroll" />
        </div>
      </div>
    </div>
  </div>
</template>
