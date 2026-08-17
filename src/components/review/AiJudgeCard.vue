<script setup lang="ts">
import { CircleCheck, LoaderCircle, Sparkles, X } from "@lucide/vue";
import { onBeforeUnmount, onMounted, ref } from "vue";
import { evaluateAnswer } from "../../services/stores/reviewStore";
import { resolveError } from "../../utils/errorMessage";
import type { ReviewCard } from "../../domain/types";

/**
 * AI 判定问答卡：用户独立作答，隐藏内部判定要点并由 AI 给出反馈。
 * 本地作答与判定状态随卡片 :key 重建自动重置。
 */
const props = defineProps<{
  card: ReviewCard;
  busy: boolean;
}>();

const emit = defineEmits<{
  /** 判定完成：上抛结果用于统计，随后由父层进入下一张。 */
  result: [isCorrect: boolean];
  error: [message: string];
  next: [];
}>();

type AiState = "idle" | "checking" | "correct" | "incorrect";
const answer = ref("");
const state = ref<AiState>("idle");
const feedback = ref("");
const suggested = ref("");

/** 提交 AI 判定。 */
async function submit(): Promise<void> {
  if (!answer.value.trim() || state.value === "checking" || props.busy) {
    return;
  }
  state.value = "checking";
  try {
    const result = await evaluateAnswer(props.card.id, answer.value, props.card.version);
    state.value = result.isCorrect ? "correct" : "incorrect";
    feedback.value = result.feedback;
    suggested.value = result.suggestedAnswer;
    emit("result", result.isCorrect);
  } catch (error) {
    state.value = "idle";
    emit("error", resolveError(error));
  }
}

/** AI 判定卡键盘：判定完成后空格进入下一题；输入框内不拦截。 */
function handleKeydown(event: KeyboardEvent): void {
  const target = event.target as HTMLElement | null;
  if (target instanceof HTMLInputElement || target instanceof HTMLTextAreaElement) {
    return;
  }
  if (event.key === " " && (state.value === "correct" || state.value === "incorrect")) {
    event.preventDefault();
    emit("next");
  }
}

onMounted(() => window.addEventListener("keydown", handleKeydown));
onBeforeUnmount(() => window.removeEventListener("keydown", handleKeydown));
</script>

<template>
  <div class="text-center">
    <p class="text-[11px] tracking-wide text-ink-3 uppercase">AI 问答</p>
    <h2 class="note-title mt-4 !text-[22px]">{{ card.front }}</h2>
  </div>

  <div class="mt-8">
    <textarea v-model="answer" class="field-textarea !min-h-28 !text-[14px] !leading-6" :disabled="state === 'checking'" placeholder="在这里写下你的回答…" @keydown.enter.exact.prevent="submit" />
    <div v-if="state === 'checking'" class="mt-3 flex items-center justify-center gap-2 text-[12px] text-ink-3">
      <LoaderCircle :size="14" class="animate-spin text-accent-strong" />AI 正在判定…
    </div>
    <div v-else-if="state === 'correct' || state === 'incorrect'" class="mt-4 rounded-lg border p-4" :class="state === 'correct' ? 'border-success/50 bg-success/8' : 'border-danger/50 bg-danger/8'">
      <p class="flex items-center gap-1.5 text-[13px] font-semibold" :class="state === 'correct' ? 'text-success' : 'text-danger'">
        <CircleCheck v-if="state === 'correct'" :size="15" />
        <X v-else :size="15" />
        {{ state === 'correct' ? '判定：正确' : '判定：还不完整' }}
      </p>
      <p class="mt-1.5 text-[12px] leading-5 text-ink-2">{{ feedback }}</p>
      <p v-if="suggested" class="mt-3 border-t border-hairline pt-3 text-[12px] leading-6 text-ink-2">
        <span class="font-medium text-ink">参考答案：</span>{{ suggested }}
      </p>
    </div>
    <div v-if="state === 'idle'" class="mt-4 flex justify-center">
      <button type="button" class="primary-btn !h-10 !px-6" :disabled="!answer.trim() || busy" @click="submit">
        <Sparkles :size="14" />提交判定
      </button>
    </div>
    <div v-else-if="state === 'correct' || state === 'incorrect'" class="mt-4 flex justify-center">
      <button type="button" class="primary-btn !h-10 !px-6" @click="emit('next')">下一题 <kbd class="kbd ml-1 !border-transparent !bg-white/20 !text-white">空格</kbd></button>
    </div>
  </div>
</template>
