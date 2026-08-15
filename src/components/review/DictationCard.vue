<script setup lang="ts">
import { Check, Volume2, X } from "@lucide/vue";
import { onBeforeUnmount, onMounted, ref } from "vue";
import { checkDictation, synthesizeSpeech } from "../../services/stores/reviewStore";
import { resolveError } from "../../utils/errorMessage";
import type { ReviewCard } from "../../domain/types";
import RatingRow from "./RatingRow.vue";

/**
 * 单词听写卡：后端权威判定拼写，支持朗读（后端合成回退浏览器 TTS）。
 * 本地状态（作答、判定结果、揭示）随卡片 :key 重建自动重置。
 */
const props = defineProps<{
  card: ReviewCard;
  busy: boolean;
}>();

const emit = defineEmits<{
  rate: [rating: "again" | "hard" | "good"];
  error: [message: string];
}>();

const answer = ref("");
const correct = ref<boolean | null>(null);
const expected = ref("");
const revealed = ref(false);
const speaking = ref(false);

/** 提交听写答案（后端权威判定）。 */
async function check(): Promise<void> {
  if (!answer.value.trim() || props.busy) {
    return;
  }
  try {
    const result = await checkDictation(props.card.id, answer.value);
    correct.value = result.correct;
    expected.value = result.expected;
    revealed.value = true;
  } catch (error) {
    emit("error", resolveError(error));
  }
}

/** 放弃作答直接显示答案。 */
function reveal(): void {
  if (correct.value === null) {
    correct.value = false;
    expected.value = props.card.back;
  }
  revealed.value = true;
}

/** 朗读单词发音：优先后端合成，失败回退浏览器 TTS。 */
async function speak(): Promise<void> {
  if (speaking.value) {
    return;
  }
  speaking.value = true;
  try {
    const dataUrl = await synthesizeSpeech(props.card.back);
    if (dataUrl) {
      await new Audio(dataUrl).play();
      speaking.value = false;
      return;
    }
  } catch {
    // 后端合成失败时回退浏览器内置语音。
  }
  const utterance = new SpeechSynthesisUtterance(props.card.back);
  utterance.lang = "en-US";
  utterance.onend = () => {
    speaking.value = false;
  };
  utterance.onerror = () => {
    speaking.value = false;
  };
  window.speechSynthesis.speak(utterance);
}

/** 听写卡键盘：空格提交/揭示，1/2/3 自评；输入框内不拦截。 */
function handleKeydown(event: KeyboardEvent): void {
  const target = event.target as HTMLElement | null;
  if (target instanceof HTMLInputElement || target instanceof HTMLTextAreaElement) {
    return;
  }
  if (event.key === " ") {
    event.preventDefault();
    if (!revealed.value) {
      void check();
    }
    return;
  }
  if (["1", "2", "3"].includes(event.key) && revealed.value) {
    emit("rate", event.key === "1" ? "again" : event.key === "2" ? "hard" : "good");
  }
}

onMounted(() => window.addEventListener("keydown", handleKeydown));
onBeforeUnmount(() => {
  window.removeEventListener("keydown", handleKeydown);
  window.speechSynthesis?.cancel();
});
</script>

<template>
  <div class="text-center">
    <p class="text-[11px] tracking-wide text-ink-3 uppercase">听写</p>
    <h2 class="note-title mt-4">{{ card.front }}</h2>
    <p v-if="card.detail" class="mt-2 font-mono text-[13px] text-ink-3">{{ card.detail }}</p>
    <button type="button" class="ghost-btn mx-auto mt-3" :disabled="speaking" @click="speak">
      <Volume2 :size="14" />{{ speaking ? "朗读中…" : "听发音" }}
    </button>
  </div>

  <div class="mt-8">
    <div class="flex items-center gap-2 rounded-lg border bg-bg-paper p-1.5 transition" :class="correct === true ? 'border-success' : correct === false ? 'border-danger' : 'border-hairline focus-within:border-accent'">
      <input v-model="answer" class="h-10 min-w-0 flex-1 bg-transparent px-2.5 text-center text-[16px] tracking-wide outline-none" :disabled="revealed" placeholder="输入单词" @keyup.enter="check" />
      <span v-if="correct !== null" class="grid size-8 shrink-0 place-items-center rounded-md text-white" :class="correct ? 'bg-success' : 'bg-danger'">
        <Check v-if="correct" :size="14" />
        <X v-else :size="14" />
      </span>
    </div>

    <div v-if="revealed" class="mt-4 rounded-lg bg-bg-side p-4 text-center">
      <p class="text-[10px] text-ink-3">正确答案</p>
      <p class="mt-1 text-[20px] font-semibold text-accent-strong">{{ expected }}</p>
      <p v-if="card.example" class="mt-2 font-serif text-[13px] leading-6 text-ink-2 italic">{{ card.example }}</p>
    </div>

    <div v-if="!revealed" class="mt-5 flex justify-center gap-2">
      <button type="button" class="ghost-btn" @click="reveal">不记得了</button>
      <button type="button" class="primary-btn" :disabled="!answer.trim() || busy" @click="check">
        <Check :size="14" />检查答案
      </button>
    </div>

    <RatingRow v-if="revealed" :busy="busy" @rate="emit('rate', $event)" />
  </div>
</template>
