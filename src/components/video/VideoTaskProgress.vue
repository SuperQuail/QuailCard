<script setup lang="ts">
import { computed, ref } from "vue";
import { CircleStop } from "@lucide/vue";
import type { VideoTaskStatus } from "../../domain/video";
import { transcriptLabel } from "../../domain/video";
import VirtualScrollbar from "../VirtualScrollbar.vue";
import VideoAsrProgress from "./VideoAsrProgress.vue";

/** 任务日志滚动容器：浮层滚动条与它同级；折叠时容器不可见，浮层自然退场。 */
const logsScroll = ref<HTMLElement | null>(null);

const props = defineProps<{ task: VideoTaskStatus }>();
const emit = defineEmits<{ stop: [] }>();

/** 运行中才显示停止按钮，终态展示结果摘要。 */
const running = computed(() => props.task.state === "running");
/** 终态文案：失败与取消给出明确原因。 */
const summary = computed(() => {
  if (props.task.state === "completed") return "已完成";
  if (props.task.state === "cancelled") return "已停止";
  if (props.task.state === "failed") return props.task.error ?? "任务失败";
  return props.task.step;
});
</script>

<template>
  <section class="space-y-3 rounded-2xl border border-hairline bg-bg-paper p-4">
    <div class="flex items-center justify-between gap-2">
      <p class="text-[13px] font-medium">{{ running ? task.step : summary }}</p>
      <button v-if="running" class="ghost-btn border border-hairline" @click="emit('stop')">
        <CircleStop :size="14" :stroke-width="1.8" />停止
      </button>
    </div>
    <VideoAsrProgress v-if="running && task.asrProgress" :progress="task.asrProgress" />
    <div v-else class="h-1.5 w-full overflow-hidden rounded-full bg-bg-active">
      <div
        class="h-full rounded-full bg-accent-strong transition-[width] duration-300"
        :style="{ width: Math.max(2, Math.min(100, task.progress)) + '%' }"
      />
    </div>
    <p class="text-[12px] text-ink-2">
      <template v-if="!task.asrProgress || !running">流程阶段 {{ task.progress }}%</template><template v-if="task.segments"> · {{ task.segments }} 段语音</template>
      <template v-if="task.transcriptSource"> · {{ transcriptLabel(task.transcriptSource) }}</template>
      <template v-if="task.shots"> · {{ task.shots }} 张截图</template>
    </p>
    <p v-if="task.backend" class="text-[12px] text-ink-3">转写后端：{{ task.backend }}</p>
    <p v-if="task.message && running" class="text-[12px] text-ink-3">{{ task.message }}</p>
    <details v-if="task.logs?.length" class="text-[12px] text-ink-3">
      <summary>任务日志</summary>
      <ul ref="logsScroll" class="soft-scrollbar max-h-40 overflow-y-auto"><li v-for="(line, index) in task.logs" :key="index">{{ line }}</li></ul>
      <VirtualScrollbar :target="logsScroll" />
    </details>
  </section>
</template>
