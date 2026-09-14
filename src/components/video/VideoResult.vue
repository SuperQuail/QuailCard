<script setup lang="ts">
import { computed } from "vue";
import { FileText, RefreshCw } from "@lucide/vue";
import type { VideoTaskStatus } from "../../domain/video";
import { transcriptLabel } from "../../domain/video";

const props = defineProps<{ task: VideoTaskStatus }>();
const emit = defineEmits<{ "open-note": [path: string]; restart: [] }>();

/** 只有成功且确实写出笔记时才展示打开入口。 */
const notePath = computed(() => props.task.notePath ?? "");
/** 失败时提示可重试。 */
const failed = computed(() => props.task.state === "failed" || props.task.state === "cancelled");
</script>

<template>
  <section class="space-y-3 rounded-2xl border border-hairline bg-bg-paper p-4">
    <div class="flex items-center gap-2 text-[13px] font-medium">
      <FileText :size="15" :stroke-width="1.8" />
      <span>生成结果</span>
    </div>
    <p class="break-all text-[12px] text-ink-2">{{ notePath || "尚未生成笔记" }}</p>
    <p class="text-[12px] text-ink-2">
      转录来源：{{ transcriptLabel(task.transcriptSource) }} · 截图 {{ task.shots }} 张
    </p>
    <div class="flex flex-wrap gap-2">
      <button v-if="notePath" class="primary-btn" @click="emit('open-note', notePath)">
        <FileText :size="15" />打开笔记
      </button>
      <button v-if="failed" class="ghost-btn border border-hairline" @click="emit('restart')">
        <RefreshCw :size="14" :stroke-width="1.8" />重新生成
      </button>
    </div>
  </section>
</template>
