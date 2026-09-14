<script setup lang="ts">
import { ref } from "vue";
import { Volume2 } from "@lucide/vue";
import { pronounce } from "../../services/pronunciation";
const props = defineProps<{ word: string }>();
const speaking = ref(false);
/** 点击朗读：合成与回退都由 pronunciation 服务处理，这里只表达进行中状态。 */
async function play(): Promise<void> {
  if (speaking.value) return;
  speaking.value = true;
  try {
    await pronounce(props.word);
  } finally {
    speaking.value = false;
  }
}
</script>
<template>
  <button type="button" class="ml-1 inline-flex size-5 shrink-0 items-center justify-center rounded-full text-ink-3 align-middle transition-colors hover:bg-bg-side hover:text-accent-strong"
    :title="`朗读 ${word}`" :aria-label="`朗读 ${word}`" @click.stop="play">
    <Volume2 :size="13" :class="{ 'animate-pulse': speaking }" />
  </button>
</template>
