<script setup lang="ts">
import { Check, Lock } from "@lucide/vue";
import { computed } from "vue";
import type { LetterMark, TypingAttempt, TypingItem, TypingRecord } from "../../domain/typingDrill";

/**
 * 单个练习格：显示释义、目标字母的红绿判定与记忆遍的模糊遮罩。
 *
 * 契约：纯展示组件，不读 store、不发请求；所有状态由父层算好后传入。
 * 记忆遍里已打出的字母直接替换到目标位置，不额外开一行，也不写「正确答案」，
 * 避免格子高度跳动或把卡片撑大。
 */
const props = defineProps<{
  item: TypingItem;
  index: number;
  state: "pending" | "active" | "done";
  attempt: TypingAttempt | null;
  record: TypingRecord | null;
}>();

interface DisplayLetter {
  char: string;
  mark: LetterMark;
  /** 记忆遍里尚未打出的目标字母：模糊显示，打出来才变清晰。 */
  sealed: boolean;
}

const chars = computed(() => [...props.item.word.word]);

/** 已完成的格子用记录里的判定，进行中的格子用当前作答，未轮到的格子不渲染内容。 */
const marks = computed<LetterMark[]>(() => {
  if (props.state === "done") return props.record?.marks ?? chars.value.map(() => "pending" as LetterMark);
  if (props.state === "active") return props.attempt?.marks ?? chars.value.map(() => "pending" as LetterMark);
  return chars.value.map(() => "pending" as LetterMark);
});

/** 目标字母与判定状态一一对应；记忆遍把已打出的位置换成用户自己的字母。 */
const letters = computed<DisplayLetter[]>(() => {
  const memoryTyping = props.item.memory && props.state === "active";
  return chars.value.map((char, index) => {
    if (memoryTyping) {
      const typed = props.attempt?.typed[index];
      if (typed) {
        return { char: typed.char, mark: typed.correct ? "correct" : "wrong", sealed: false };
      }
      return { char, mark: "pending" as LetterMark, sealed: true };
    }
    return { char, mark: marks.value[index] ?? "pending", sealed: false };
  });
});

/** 记忆遍在完成前不暴露释义与音标；原词在格子里逐字母模糊。 */
const sealed = computed(() => props.item.memory && props.state !== "done");
const perfect = computed(() => props.state === "done" && props.record?.summary.perfect === true);
const passLabel = computed(() => (props.item.memory ? "记忆遍" : `第 ${props.item.pass} 遍`));
const label = computed(() => `${props.index} 号格，${passLabel.value}，中文：${props.item.word.meaning}`);
</script>

<template>
  <article
    class="typing-cell"
    :class="[`is-${state}`, { 'is-perfect': perfect, 'is-memory': item.memory }]"
    :data-item-id="item.id"
    :aria-label="label"
  >
    <header class="typing-cell-head">
      <span class="typing-cell-index">{{ index }}</span>
      <Lock v-if="item.memory && state !== 'done'" :size="11" aria-hidden="true" />
      <span class="typing-cell-pass">{{ passLabel }}</span>
      <Check v-if="perfect" :size="12" class="text-success" aria-hidden="true" />
    </header>

    <template v-if="state === 'pending'">
      <p class="typing-cell-empty">等待练习</p>
    </template>
    <template v-else>
      <p class="typing-cell-meaning" :class="{ 'is-blurred': sealed }">{{ item.word.meaning }}</p>
      <p v-if="item.word.phonetic" class="typing-cell-phonetic" :class="{ 'is-blurred': sealed }">{{ item.word.phonetic }}</p>
      <p class="typing-cell-word" :aria-hidden="sealed">
        <span
          v-for="(letter, letterIndex) in letters"
          :key="letterIndex"
          class="typing-cell-letter"
          :class="[`is-${letter.mark}`, { 'is-sealed': letter.sealed }]"
        >{{ letter.char }}</span>
      </p>
    </template>
  </article>
</template>
