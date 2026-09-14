<script setup lang="ts">
import { onMounted } from "vue";
import type { ReviewSessionFlow } from "../../services/reviewSession";
import ReviewCardHost from "../review/ReviewCardHost.vue";
const props = defineProps<{ flow: ReviewSessionFlow; aiGrading: boolean }>();
const emit = defineEmits<{ summary: [text: string]; generate: [] }>();
const { loading, errorMessage, busy, queue, index, finished, stats, outcomes, currentCard, loadQueue, rate, evaluate, nextCard } = props.flow;
/** 挂载只加载真实复习队列，评分必须由卡片内用户动作触发。 */
onMounted(() => { void loadQueue(); });
/** 汇总以本轮已提交的统计为准，用户点击后才交给 Agent 继续讲解。 */
function discuss(): void {
  const weak = outcomes.value.filter(item => item.rating !== "good").map(item => `${item.question}（${item.notePath}，${item.rating === 'again' ? '忘记' : '困难'}）${item.answer ? `，我的回答：${item.answer}` : ''}`).join("\n");
  emit("summary", `本轮正式复习完成：记得 ${stats.good} 张、困难 ${stats.hard} 张、忘记 ${stats.again} 张。请带我回顾薄弱点，一次讲解一个考点。\n${weak || '本轮都记得，可用一个迁移问题检查我的理解。'}`);
}
</script>
<template>
  <section class="my-4 rounded-xl border border-hairline bg-bg-paper p-5" aria-label="对话内正式复习">
    <header class="mb-5 flex justify-between text-[12px] text-ink-3"><span>正式复习 · 计入学习进度</span><span>{{ finished ? queue.length : Math.min(index + 1, queue.length) }} / {{ queue.length }}</span></header>
    <p v-if="loading" class="text-[13px]">正在读取复习卡…</p>
    <div v-else-if="finished" class="space-y-3 text-center">
      <p class="text-[16px] font-medium">本轮复习完成</p>
      <p class="text-[13px] text-ink-2">记得 {{ stats.good }} · 困难 {{ stats.hard }} · 忘记 {{ stats.again }}</p>
      <button class="primary-btn" @click="discuss">和 Agent 回顾薄弱点</button>
    </div>
    <div v-else-if="!queue.length" class="space-y-3 text-[13px]">
      <p>{{ errorMessage || '当前范围没有可复习的卡片。可以先学习笔记，或生成学习卡片。' }}</p>
      <button class="ghost-btn" @click="emit('generate')">让 Agent 帮我生成卡片</button>
    </div>
    <ReviewCardHost v-else-if="currentCard" :card="currentCard" :busy="busy" :ai-grading="aiGrading" :evaluate="evaluate"
      @rate="rate" @next="nextCard" @error="errorMessage = $event" />
    <p v-if="errorMessage && queue.length" role="alert" class="mt-3 text-[12px] text-danger">{{ errorMessage }}</p>
  </section>
</template>
