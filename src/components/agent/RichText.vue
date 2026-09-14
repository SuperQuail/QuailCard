<script setup lang="ts">
import { computed } from "vue";
import { splitInline } from "../../markdown/model";
import { splitPhonetics } from "../../domain/phonetic";
import MathFormula from "./MathFormula.vue";
import PhoneticAudio from "./PhoneticAudio.vue";
const props = defineProps<{ text: string; fallbackWord?: string }>();
const emit = defineEmits<{ link: [href: string] }>();
/** 行内标记负责排版，音标负责朗读按钮，两者在同一次遍历里合并。 */
interface RichPiece { text: string; kind?: "bold" | "italic" | "code" | "strike" | "link" | "math"; href?: string; display?: boolean; word?: string }
/** 站内笔记链接：只有指向 .md 的目标才是跳转入口，其余链接按普通文字显示。 */
function isNoteLink(href: string | undefined): boolean {
  return Boolean(href && /\.md$/i.test(href));
}
const pieces = computed<RichPiece[]>(() => {
  const result: RichPiece[] = [];
  for (const span of splitInline(props.text)) {
    // 代码、链接与公式不参与音标拆分：它们要整体渲染，不能被读音标拆开。
    if (span.kind === "code" || span.kind === "link" || span.kind === "math") {
      result.push({ text: span.text, kind: span.kind, href: span.href, display: span.display });
      continue;
    }
    for (const piece of splitPhonetics(span.text)) {
      // 音标常单独成段（「音标: /…/」），段内找不到单词时沿用上一段最近的英文词。
      const word = piece.word ?? (piece.phonetic ? props.fallbackWord : undefined);
      result.push({ text: piece.text, kind: span.kind, word });
    }
  }
  return result;
});
</script>
<template>
  <template v-for="(piece, index) in pieces" :key="index">
    <button
      v-if="piece.kind === 'link' && isNoteLink(piece.href)"
      type="button"
      class="text-accent-strong underline underline-offset-4"
      @click="emit('link', piece.href ?? '')"
    >{{ piece.text }}</button>
    <MathFormula v-else-if="piece.kind === 'math'" :tex="piece.text" :display="piece.display" />
    <code v-else-if="piece.kind === 'code'" class="rounded bg-code-bg px-1 text-[0.92em]">{{ piece.text }}</code>
    <s v-else-if="piece.kind === 'strike'">{{ piece.text }}</s>
    <strong v-else-if="piece.kind === 'bold'">{{ piece.text }}<PhoneticAudio v-if="piece.word" :word="piece.word" /></strong>
    <em v-else-if="piece.kind === 'italic'">{{ piece.text }}<PhoneticAudio v-if="piece.word" :word="piece.word" /></em>
    <template v-else>{{ piece.text }}<PhoneticAudio v-if="piece.word" :word="piece.word" /></template>
  </template>
</template>
