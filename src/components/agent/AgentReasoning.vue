<script setup lang="ts">
import { computed, ref } from "vue";
import { Brain, ChevronRight } from "@lucide/vue";
const props = defineProps<{ text: string; running: boolean }>();
const open = ref(false);
/** 折叠摘要：运行中跟随最后一行，结束后显示首行；去掉 Markdown 强调符。 */
const summary = computed(() => {
  const text = props.text.trimEnd();
  const line = props.running ? text.slice(text.lastIndexOf("\n") + 1) : (text.split("\n")[0] ?? "");
  return line.replace(/\*\*/g, "");
});
</script>
<template>
  <div class="min-w-0" data-variant="think" :data-state="running ? 'running' : 'ok'">
    <button type="button" class="flex w-full min-w-0 items-center gap-2 rounded-lg px-2 py-1 text-left text-[12px] text-ink-3 transition-colors hover:bg-bg-side"
      :aria-expanded="open" aria-label="思考过程" @click="open = !open">
      <Brain :size="14" class="shrink-0" />
      <span class="shrink-0">思考</span>
      <span class="min-w-0 flex-1 truncate text-ink-2">{{ summary }}</span>
      <ChevronRight :size="14" class="shrink-0 transition-transform" :class="{ 'rotate-90': open }" />
    </button>
    <div v-if="open" class="mt-1 max-h-80 overflow-y-auto whitespace-pre-wrap break-words rounded-lg border border-hairline bg-bg-side px-3 py-2 text-[12px] leading-5 text-ink-2">{{ text }}</div>
  </div>
</template>
