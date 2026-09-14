<script setup lang="ts">
import { computed, watch } from "vue";
import { useTypingText } from "../../composables/useTypingText";
import { listMarker, parseNoteBlocks } from "../../markdown/model";
import type { NoteBlock } from "../../markdown/types";
import { latinWords } from "../../domain/phonetic";
import AgentMarkdownTable from "./AgentMarkdownTable.vue";
import RichText from "./RichText.vue";
const props = defineProps<{ text: string; animate?: boolean }>();
const emit = defineEmits<{ note: [path: string]; progress: [] }>();
const displayed = useTypingText(() => props.text, () => !!props.animate);
/** displayed 在流式期间每帧只发布一次；缓存解析结果供正文和音标共用，不直接订阅网络 text。 */
const blocks = computed(() => parseNoteBlocks(displayed.value));
/** 供朗读兜底使用的块文字；列表与表格也要参与，避免音标段落丢掉上下文。 */
function blockWords(block: NoteBlock): string {
  switch (block.type) {
    case "list":
      return block.items.map((item) => item.text).join(" ");
    case "table":
      return [...block.header, ...block.rows.flat()].join(" ");
    case "hr":
      return "";
    default:
      return block.text;
  }
}
/** 音标常单独成段，这里把上一段最近的英文词作为朗读兜底，段内已找到的优先。 */
const fallbackWords = computed(() => {
  const result: Array<string | undefined> = [];
  let current: string | undefined;
  for (const block of blocks.value) {
    result.push(current);
    const words = latinWords(blockWords(block));
    if (words.length) current = words[words.length - 1];
  }
  return result;
});
/** 站内笔记跳转由 AgentText 统一发出；行内链接只负责报告点击目标。 */
function openNote(path: string): void {
  emit("note", path);
}
/** 与已发布的显示帧同步且在 DOM 更新后通知滚动；同帧输入突发不会逐块发 progress。 */
watch(displayed, () => emit("progress"), { flush: "post" });
</script>
<template>
  <div class="min-w-0 space-y-4 text-[16px] leading-7 text-ink [overflow-wrap:anywhere]">
    <template v-for="(block, index) in blocks" :key="index">
      <hr v-if="block.type === 'hr'" class="border-hairline" />
      <AgentMarkdownTable
        v-else-if="block.type === 'table'"
        :header="block.header"
        :rows="block.rows"
        :alignments="block.alignments"
        :fallback-word="fallbackWords[index]"
        @link="openNote"
      />
      <ul v-else-if="block.type === 'list'" class="list-none space-y-1 p-0">
        <li
          v-for="(item, itemIndex) in block.items"
          :key="itemIndex"
          class="flex gap-2"
          :style="{ paddingLeft: `${item.depth * 20}px` }"
        >
          <span class="shrink-0 text-ink-3">{{ listMarker(item) }}</span>
          <span class="min-w-0 flex-1"><RichText :text="item.text" :fallback-word="fallbackWords[index]" @link="openNote" /></span>
        </li>
      </ul>
      <pre v-else-if="block.type === 'code'" class="max-w-full overflow-x-auto rounded-xl border border-hairline bg-code-bg p-4 text-[13px] leading-6">{{ block.text }}</pre>
      <div
        v-else
        class="whitespace-pre-wrap"
        :class="{ 'font-semibold text-ink': block.type === 'heading', 'border-l-2 border-accent pl-3': block.type === 'quote' }"
      >
        <RichText :text="block.text" :fallback-word="fallbackWords[index]" @link="openNote" />
      </div>
    </template>
  </div>
</template>
