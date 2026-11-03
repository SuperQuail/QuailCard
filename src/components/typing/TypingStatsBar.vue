<script setup lang="ts">
import { Flame, Trophy, Zap } from "@lucide/vue";
import { computed } from "vue";

/** 练习统计条：连击、最高连击、得分、命中率与速度。 */
const props = defineProps<{
  combo: number;
  maxCombo: number;
  score: number;
  accuracy: number;
  speed: number;
  comboMilestone: boolean;
}>();

/** 命中率只做展示取整，计算口径在领域层。 */
const accuracyPercent = computed(() => Math.round(props.accuracy * 100));
</script>

<template>
  <div class="flex flex-wrap items-center gap-x-4 gap-y-1 text-[11px] text-ink-3" aria-live="polite">
    <span class="flex items-center gap-1"><Flame :size="13" class="text-warning" />连击
      <b :key="combo" class="inline-block tabular-nums text-ink" :class="{ 'qc-combo-pulse': comboMilestone }">{{ combo }}</b>
    </span>
    <span class="flex items-center gap-1"><Trophy :size="13" />最高 <b class="tabular-nums text-ink">{{ maxCombo }}</b></span>
    <span class="flex items-center gap-1"><Zap :size="13" />得分 <b class="tabular-nums text-ink">{{ score }}</b></span>
    <span>命中 <b class="tabular-nums text-ink">{{ accuracyPercent }}%</b></span>
    <span>速度 <b class="tabular-nums text-ink">{{ speed }}</b> 字母/分</span>
  </div>
</template>
