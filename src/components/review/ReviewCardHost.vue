<script setup lang="ts">
import { computed } from "vue";
import type { AiEvaluationResult, ReviewCard, ReviewRating } from "../../domain/types";
import AiJudgeCard from "./AiJudgeCard.vue";
import DictationCard from "./DictationCard.vue";
import SelfReviewCard from "./SelfReviewCard.vue";
const props = defineProps<{ card: ReviewCard; busy: boolean; suspended?: boolean; aiGrading: boolean; evaluate: (id: string, answer: string, version: number) => Promise<AiEvaluationResult> }>();
const emit = defineEmits<{ rate: [rating: ReviewRating]; error: [message: string]; next: [] }>();
/** 仅把组件需要的属性和事件交给对应学习方式。 */
function commonProps() { return { card: props.card, busy: props.busy, suspended: props.suspended }; }
/** 评分事件属于用户动作，由外层共享用例执行。 */
function rate(rating: ReviewRating): void { emit("rate", rating); }
/** 子卡片只上报安全错误消息。 */
function error(message: string): void { emit("error", message); }
/** 下一题只推进已经提交的当前卡片。 */
function next(): void { emit("next"); }
const manual = { component: SelfReviewCard, bindings: commonProps, events: { rate } };
const dictation = { component: DictationCard, bindings: commonProps, events: { rate, error } };
const ai = { component: AiJudgeCard, bindings: () => ({ ...commonProps(), evaluate: props.evaluate }), events: { error, next } };
/** 注册已有学习方式，新增卡片展示不扩展模板条件链。 */
const modes = { vocabulary: { manual: dictation, ai: dictation }, qa: { manual, ai }, ai: { manual, ai } };
const entry = computed(() => modes[props.card.kind][props.aiGrading ? "ai" : "manual"]);
</script>
<template>
  <component :is="entry.component" :key="`${card.id}:${card.version}`" v-bind="entry.bindings()" v-on="entry.events" />
</template>
