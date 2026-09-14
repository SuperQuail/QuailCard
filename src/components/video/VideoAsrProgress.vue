<script setup lang="ts">
import { computed } from "vue";
import type { AsrProgress } from "../../domain/video";
import { formatDuration } from "../../domain/video";
import { asrMeasurements } from "../../domain/videoAsrProgress";

const props = defineProps<{ progress: AsrProgress }>();
/** 展示后端最近一次真实回调，不用前端计时补间百分比或推测 ETA。 */
const metrics = computed(() => asrMeasurements(props.progress));
</script>

<template>
  <div class="space-y-1 text-[12px] text-ink-2" data-testid="asr-progress">
    <p>Whisper 本地转写 P{{ progress.page }}<template v-if="progress.attempt > 1"> · CPU 重试</template></p>
    <p v-if="metrics.percent === null">等待 Whisper 报告进度，暂无法计算速度</p>
    <template v-else>
      <progress class="h-1.5 w-full accent-accent-strong" aria-label="当前分 P Whisper 转写进度" :value="metrics.percent" max="100" />
      <p>Whisper {{ metrics.percent }}%<template v-if="metrics.processed !== null && metrics.total !== null"> · 音频约 {{ formatDuration(metrics.processed) }} / {{ formatDuration(metrics.total) }}</template></p>
      <p>本次观测耗时 {{ formatDuration(metrics.elapsed) }}<template v-if="metrics.speed !== null"> · 平均约 {{ metrics.speed.toFixed(2) }}×（音频秒/秒）</template></p>
      <p class="text-ink-3">按组件进度更新；音频量与平均速度由视频时长估算，非实时瞬时速度。</p>
    </template>
  </div>
</template>
