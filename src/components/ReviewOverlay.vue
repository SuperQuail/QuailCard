<script setup lang="ts">
import { ArrowLeft, CircleCheck } from "@lucide/vue";
import { computed, onBeforeUnmount, onMounted, ref } from "vue";
import { aiGradingEnabled, getReviewQueue, submitReview } from "../services/stores/reviewStore";
import { resolveError } from "../utils/errorMessage";
import type { ReviewCard } from "../domain/types";
import AiJudgeCard from "./review/AiJudgeCard.vue";
import DictationCard from "./review/DictationCard.vue";
import SelfReviewCard from "./review/SelfReviewCard.vue";

/**
 * 复习会话编排：加载队列、切换卡片、汇总统计与完成页。
 * 单卡交互（听写/自评/AI 判定与各自键盘快捷键）在三个卡片子组件内。
 */
const props = defineProps<{
  title: string;
  notePath: string | null;
  includeAll: boolean;
}>();

const emit = defineEmits<{ close: [] }>();

type Rating = "again" | "hard" | "good";

const loading = ref(true);
const errorMessage = ref("");
const queue = ref<ReviewCard[]>([]);
const index = ref(0);
const stats = ref<Record<Rating, number>>({ again: 0, hard: 0, good: 0 });
const finished = ref(false);
const busy = ref(false);

const currentCard = computed(() => queue.value[index.value] ?? null);
/** 问答卡是否使用 AI 评分：由全局设置控制。 */
const useAiGrading = computed(() => currentCard.value !== null && currentCard.value.kind !== "vocabulary" && aiGradingEnabled.value);
/** 当前进度文案。 */
const progress = computed(() => finished.value ? `${queue.value.length} / ${queue.value.length}` : `${index.value + 1} / ${queue.value.length}`);

/** 从后端加载复习队列。 */
async function loadQueue(): Promise<void> {
  loading.value = true;
  errorMessage.value = "";
  try {
    queue.value = await getReviewQueue(props.notePath, props.includeAll);
    if (queue.value.length === 0) {
      errorMessage.value = "当前队列没有卡片";
    }
  } catch (error) {
    errorMessage.value = resolveError(error);
  } finally {
    loading.value = false;
  }
}

/** 提交评分并进入下一张。 */
async function rate(rating: Rating): Promise<void> {
  const card = currentCard.value;
  if (!card || busy.value) {
    return;
  }
  busy.value = true;
  try {
    await submitReview(card.id, rating, card.version);
    stats.value[rating] += 1;
    nextCard();
  } catch (error) {
    errorMessage.value = resolveError(error);
  } finally {
    busy.value = false;
  }
}

/** AI 判定完成：按结果计入统计，等待用户按键进入下一张。 */
function handleAiResult(isCorrect: boolean): void {
  stats.value[isCorrect ? "good" : "again"] += 1;
}

/** 进入下一张卡片或完成本轮。 */
function nextCard(): void {
  if (index.value + 1 >= queue.value.length) {
    finished.value = true;
    return;
  }
  index.value += 1;
}

/** 再来一轮。 */
function restartRound(): void {
  index.value = 0;
  finished.value = false;
  stats.value = { again: 0, hard: 0, good: 0 };
}

/** 会话级键盘：Esc 返回（单卡快捷键由子组件监听）。 */
function handleKeydown(event: KeyboardEvent): void {
  if (event.key === "Escape") {
    emit("close");
  }
}

onMounted(() => {
  void loadQueue();
  window.addEventListener("keydown", handleKeydown);
});

onBeforeUnmount(() => window.removeEventListener("keydown", handleKeydown));
</script>

<template>
  <div class="fixed inset-0 z-50 flex flex-col bg-bg">
    <!-- 顶栏：返回、标题、进度 -->
    <header class="flex h-12 shrink-0 items-center gap-3 border-b border-hairline px-3">
      <button type="button" class="ghost-btn" @click="emit('close')">
        <ArrowLeft :size="14" />返回
      </button>
      <p class="min-w-0 flex-1 truncate text-center text-[12px] font-medium">{{ title }}</p>
      <span class="text-[11px] tabular-nums text-ink-3">{{ progress }}</span>
    </header>
    <div class="h-0.5 shrink-0 bg-bg-side">
      <div class="h-full bg-accent transition-all" :style="{ width: `${(Math.min(index + 1, queue.length) / Math.max(queue.length, 1)) * 100}%` }" />
    </div>

    <!-- 加载与空队列 -->
    <section v-if="loading || queue.length === 0" class="flex min-h-0 flex-1 items-center justify-center px-6">
      <div class="flex flex-col items-center text-center">
        <p class="text-[14px] font-medium">{{ loading ? "正在加载复习队列" : errorMessage || "当前队列没有卡片" }}</p>
        <button type="button" class="ghost-btn mt-4" @click="emit('close')">返回笔记</button>
      </div>
    </section>

    <!-- 完成页 -->
    <section v-else-if="finished" class="flex min-h-0 flex-1 items-center justify-center px-6">
      <div class="flex flex-col items-center text-center">
        <CircleCheck :size="34" class="text-success" />
        <h2 class="mt-4 text-[18px] font-semibold">本轮复习完成</h2>
        <p class="mt-1 text-[12px] text-ink-3">共复习 {{ queue.length }} 张卡片</p>
        <div class="mt-6 grid grid-cols-3 gap-3 text-center">
          <div class="rounded-lg bg-bg-side px-5 py-3">
            <p class="text-[18px] font-semibold tabular-nums text-success">{{ stats.good }}</p>
            <p class="mt-0.5 text-[10px] text-ink-3">记得</p>
          </div>
          <div class="rounded-lg bg-bg-side px-5 py-3">
            <p class="text-[18px] font-semibold tabular-nums text-warning">{{ stats.hard }}</p>
            <p class="mt-0.5 text-[10px] text-ink-3">困难</p>
          </div>
          <div class="rounded-lg bg-bg-side px-5 py-3">
            <p class="text-[18px] font-semibold tabular-nums text-danger">{{ stats.again }}</p>
            <p class="mt-0.5 text-[10px] text-ink-3">忘记</p>
          </div>
        </div>
        <div class="mt-8 flex gap-2">
          <button type="button" class="ghost-btn" @click="emit('close')">返回笔记</button>
          <button type="button" class="primary-btn" @click="restartRound">再来一轮</button>
        </div>
      </div>
    </section>

    <!-- 卡片内容：换卡时以卡片 id 作 key，子组件本地状态自动重建 -->
    <section v-else-if="currentCard" class="flex min-h-0 flex-1 flex-col items-center justify-center px-6">
      <div class="w-full max-w-[640px]">
        <DictationCard
          v-if="currentCard.kind === 'vocabulary'"
          :key="currentCard.id"
          :card="currentCard"
          :busy="busy"
          @rate="(rating) => void rate(rating)"
          @error="(message) => (errorMessage = message)"
        />
        <SelfReviewCard
          v-else-if="!useAiGrading"
          :key="currentCard.id"
          :card="currentCard"
          :busy="busy"
          @rate="(rating) => void rate(rating)"
        />
        <AiJudgeCard
          v-else
          :key="currentCard.id"
          :card="currentCard"
          :busy="busy"
          @result="handleAiResult"
          @error="(message) => (errorMessage = message)"
          @next="nextCard"
        />

        <p v-if="errorMessage && !loading" class="mt-4 text-center text-[11px] text-danger">{{ errorMessage }}</p>
      </div>
    </section>
  </div>
</template>
