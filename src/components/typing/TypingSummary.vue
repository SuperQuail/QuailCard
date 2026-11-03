<script setup lang="ts">
import { Trophy } from "@lucide/vue";
import { computed } from "vue";
import type { PracticeSettings } from "../../domain/types";

/** 结算页：展示本轮成绩，并给出再来一轮 / 调整设置 / 返回三个出口。 */
const props = defineProps<{
  wordCount: number;
  pageCount: number;
  score: number;
  maxCombo: number;
  accuracy: number;
  speed: number;
  perfectWords: number;
  settings: PracticeSettings;
}>();

const emit = defineEmits<{ restart: []; adjust: []; close: [] }>();

/** 命中率只做展示取整，计算口径在领域层。 */
const accuracyPercent = computed(() => Math.round(props.accuracy * 100));
</script>

<template>
  <div class="flex min-h-0 flex-1 items-center justify-center px-6">
    <div class="w-full max-w-[520px] text-center">
      <Trophy :size="34" class="mx-auto text-warning" />
      <h2 class="mt-4 text-[18px] font-semibold">本轮打字背诵完成</h2>
      <p class="mt-1 text-[12px] text-ink-3">
        {{ wordCount }} 个单词 · 每词 {{ settings.repeatsPerWord }} 遍 + 1 遍记忆 · {{ pageCount }} 页
      </p>
      <div class="mt-6 grid grid-cols-2 gap-3 sm:grid-cols-4">
        <div class="rounded-lg bg-bg-side px-3 py-3">
          <p class="text-[18px] font-semibold tabular-nums">{{ score }}</p>
          <p class="mt-0.5 text-[10px] text-ink-3">得分</p>
        </div>
        <div class="rounded-lg bg-bg-side px-3 py-3">
          <p class="text-[18px] font-semibold tabular-nums">{{ maxCombo }}</p>
          <p class="mt-0.5 text-[10px] text-ink-3">最高连击</p>
        </div>
        <div class="rounded-lg bg-bg-side px-3 py-3">
          <p class="text-[18px] font-semibold tabular-nums">{{ accuracyPercent }}%</p>
          <p class="mt-0.5 text-[10px] text-ink-3">命中率</p>
        </div>
        <div class="rounded-lg bg-bg-side px-3 py-3">
          <p class="text-[18px] font-semibold tabular-nums">{{ speed }}</p>
          <p class="mt-0.5 text-[10px] text-ink-3">字母/分</p>
        </div>
      </div>
      <p class="mt-3 text-[11px] text-ink-3">完美词 {{ perfectWords }} / {{ wordCount }} · 练习成绩不写入复习计划</p>
      <div class="mt-6 flex flex-wrap justify-center gap-2">
        <button type="button" class="ghost-btn" @click="emit('adjust')">调整设置</button>
        <button type="button" class="ghost-btn" @click="emit('close')">返回笔记</button>
        <button type="button" class="primary-btn" @click="emit('restart')">再来一轮</button>
      </div>
    </div>
  </div>
</template>
