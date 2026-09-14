<script setup lang="ts">
import { computed } from "vue";
import { FileText, Sparkles } from "@lucide/vue";
import type { AgentMessage } from "../../domain/agent";
const props = defineProps<{ message: AgentMessage; disabled?: boolean }>();
const emit = defineEmits<{ action: [name: string, value: string] }>();
/** 工具块动作注册表只允许受控入口，不执行模型返回的任意命令。 */
const actions: Record<string, { label: string; action: string; field: string }> = {
  note: { label: "打开笔记", action: "note", field: "path" },
  change: { label: "查看差异 / 撤销", action: "change", field: "changeId" },
  generate: { label: "生成并选择卡片", action: "generate", field: "path" },
  memory: { label: "保存这条记忆", action: "memory", field: "content" },
};
const entry = computed(() => actions[props.message.kind]);
</script>
<template>
  <div class="my-3 rounded-lg border border-hairline bg-bg-side px-4 py-3">
    <p class="flex items-center gap-2 text-[12px] font-medium"><FileText :size="14" />{{ message.content }}</p>
    <p v-if="message.data?.front" class="mt-1 break-words text-[13px] leading-6">{{ message.data.front }}</p>
    <p v-if="message.data?.path" class="mt-1 break-all text-[12px] text-ink-3">{{ message.data.path }}</p>
    <p v-if="message.data?.state === 'failed'" class="mt-1 text-[12px] text-ink-3">未应用此改动</p>
    <p v-if="message.kind === 'memory'" class="mt-2 whitespace-pre-wrap text-[13px] leading-6">{{ message.data?.content }}</p>
    <button v-if="entry" type="button" class="ghost-btn mt-2" :disabled="disabled || message.data?.state === 'failed'" @click="emit('action', entry.action, String(message.data?.[entry.field] ?? ''))"><Sparkles :size="13" />{{ entry.label }}</button>
  </div>
</template>
