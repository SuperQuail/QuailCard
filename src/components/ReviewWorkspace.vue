<script setup lang="ts">
import { ArrowLeft, CircleCheck } from "@lucide/vue";
import { computed, onBeforeUnmount, onMounted, ref } from "vue";
import { aiGradingEnabled } from "../services/stores/reviewStore";
import { createReviewSession } from "../services/reviewSession";
import ReviewCardHost from "./review/ReviewCardHost.vue";
import VirtualScrollbar from "./VirtualScrollbar.vue";

/** 工作区滚动容器：浮层滚动条必须与它同级，因此根部多一层定位壳。 */
const reviewScroll = ref<HTMLElement | null>(null);

/**
 * 工作区复习：隐藏时保留队列与作答状态，暂停全局快捷键。
 * 单卡交互（听写/自评/AI 判定与各自键盘快捷键）在三个卡片子组件内。
 */
const props = defineProps<{
  title: string;
  notePath: string | null;
  includeAll: boolean;
  suspended?: boolean;
}>();

const emit = defineEmits<{ close: [] }>();

const { loading, errorMessage, queue, index, stats, finished, busy, currentCard, loadQueue, rate, evaluate, nextCard, restartRound } = createReviewSession({
  paths: props.notePath ? [props.notePath] : [], includeAll: props.includeAll, id: crypto.randomUUID(),
});
/** 显示已提交的真实复习进度。 */
const progress = computed(() => `${finished.value ? queue.value.length : Math.min(index.value + 1, queue.value.length)} / ${queue.value.length}`);

/** 会话级键盘：Esc 返回（单卡快捷键由子组件监听）。 */
function handleKeydown(event: KeyboardEvent): void {
  if (props.suspended) return;
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
  <main class="relative flex min-h-0 min-w-0 flex-1" aria-label="复习工作区">
    <div ref="reviewScroll" class="soft-scrollbar flex min-h-0 min-w-0 flex-1 flex-col overflow-y-auto bg-bg-paper">
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
        <ReviewCardHost :card="currentCard" :busy="busy" :suspended="suspended" :ai-grading="aiGradingEnabled" :evaluate="evaluate"
          @rate="rate" @error="errorMessage = $event" @next="nextCard" />

        <p v-if="errorMessage && !loading" class="mt-4 text-center text-[11px] text-danger">{{ errorMessage }}</p>
      </div>
    </section>
    </div>
    <VirtualScrollbar :target="reviewScroll" />
  </main>
</template>
