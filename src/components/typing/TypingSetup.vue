<script setup lang="ts">
import { Keyboard, Minus, Plus } from "@lucide/vue";
import { computed } from "vue";
import type { PracticeSettings } from "../../domain/types";
import type { TypingAudioProgress, TypingScope } from "../../services/typingSession";

/**
 * 练习准备页：范围、每词遍数与每页单词数。
 * 契约：只上抛新值；偏好持久化、分页重建与发音预取都由工作区会话负责。
 * 发音预取完成前开始按钮保持锁定，避免刚进练习就没有朗读。
 */
const props = defineProps<{
  scope: TypingScope;
  wordCount: number;
  settings: PracticeSettings;
  saving: boolean;
  error: string;
  canSwitchScope: boolean;
  audioReady: boolean;
  audioProgress: TypingAudioProgress;
  /** 「当前笔记」那种范围的叫法：单词组是「本组」，Markdown 笔记是「本笔记」。 */
  noteLabel?: string;
}>();

const emit = defineEmits<{
  start: [];
  scope: [scope: TypingScope];
  settings: [settings: PracticeSettings];
}>();

const MIN_REPEATS = 1;
const MAX_REPEATS = 10;
const MIN_WORDS = 1;
const MAX_WORDS = 6;

/** 开始按钮文案：发音没就绪时显示预取进度。 */
const startLabel = computed(() => {
  if (props.saving) return "保存设置…";
  if (!props.audioReady && props.wordCount > 0) return `准备发音 ${props.audioProgress.done}/${props.audioProgress.total}`;
  return "开始练习";
});

/** 步进器只发新对象，避免直接改父层状态。 */
function step(field: "repeatsPerWord" | "wordsPerPage", delta: number): void {
  const min = field === "repeatsPerWord" ? MIN_REPEATS : MIN_WORDS;
  const max = field === "repeatsPerWord" ? MAX_REPEATS : MAX_WORDS;
  const next = { ...props.settings };
  next[field] = Math.min(max, Math.max(min, next[field] + delta));
  emit("settings", next);
}
</script>

<template>
  <div class="mx-auto w-full max-w-[560px]">
    <section class="rounded-xl border border-hairline bg-bg-paper p-5">
      <p class="text-[11px] tracking-wide text-ink-3 uppercase">打字背诵</p>
      <h1 class="mt-2 text-[18px] font-semibold">看中文，把英文单词打出来</h1>

      <div v-if="canSwitchScope" class="mt-4 flex gap-1 rounded-lg bg-bg-side p-1">
        <button
          type="button"
          class="h-7 flex-1 rounded-md text-[12px] transition"
          :class="scope === 'note' ? 'bg-bg-paper font-medium shadow-sm' : 'text-ink-3'"
          @click="emit('scope', 'note')"
        >{{ noteLabel ?? "本笔记" }}</button>
        <button
          type="button"
          class="h-7 flex-1 rounded-md text-[12px] transition"
          :class="scope === 'all' ? 'bg-bg-paper font-medium shadow-sm' : 'text-ink-3'"
          @click="emit('scope', 'all')"
        >全部单词卡</button>
      </div>

      <p class="mt-4 text-[12px] text-ink-2">当前范围共 <b class="tabular-nums">{{ wordCount }}</b> 个单词</p>

      <div class="mt-4">
        <div class="setting-row">
          <div>
            <p class="text-[12px] font-medium">每词重复遍数</p>
            <p class="mt-0.5 text-[11px] text-ink-3">再加最后一遍记忆遍</p>
          </div>
          <div class="flex items-center gap-1">
            <button type="button" class="icon-btn border border-hairline !size-7" :disabled="settings.repeatsPerWord <= MIN_REPEATS || saving" aria-label="减少遍数" @click="step('repeatsPerWord', -1)">
              <Minus :size="12" />
            </button>
            <span class="w-6 text-center text-[13px] tabular-nums">{{ settings.repeatsPerWord }}</span>
            <button type="button" class="icon-btn border border-hairline !size-7" :disabled="settings.repeatsPerWord >= MAX_REPEATS || saving" aria-label="增加遍数" @click="step('repeatsPerWord', 1)">
              <Plus :size="12" />
            </button>
          </div>
        </div>
        <div class="setting-row">
          <div>
            <p class="text-[12px] font-medium">每页单词数</p>
            <p class="mt-0.5 text-[11px] text-ink-3">一页填满后自动翻页</p>
          </div>
          <div class="flex items-center gap-1">
            <button type="button" class="icon-btn border border-hairline !size-7" :disabled="settings.wordsPerPage <= MIN_WORDS || saving" aria-label="减少每页单词数" @click="step('wordsPerPage', -1)">
              <Minus :size="12" />
            </button>
            <span class="w-6 text-center text-[13px] tabular-nums">{{ settings.wordsPerPage }}</span>
            <button type="button" class="icon-btn border border-hairline !size-7" :disabled="settings.wordsPerPage >= MAX_WORDS || saving" aria-label="增加每页单词数" @click="step('wordsPerPage', 1)">
              <Plus :size="12" />
            </button>
          </div>
        </div>
      </div>

      <p v-if="error" class="mt-3 text-[11px] text-danger">{{ error }}</p>

      <button type="button" class="primary-btn mt-5 w-full justify-center" :disabled="wordCount === 0 || saving || !audioReady" @click="emit('start')">
        <Keyboard :size="14" />{{ startLabel }}
      </button>
    </section>
  </div>
</template>
