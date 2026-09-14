<script setup lang="ts">
import { computed, ref } from "vue";
import type { AgentMessage } from "../../domain/agent";
import type { GeneratedCard } from "../../domain/types";
import { isPhonetic } from "../../domain/phonetic";
import PhoneticAudio from "./PhoneticAudio.vue";
import VirtualScrollbar from "../VirtualScrollbar.vue";

/** 草稿列表滚动容器：浮层滚动条与它同级。 */
const draftsScroll = ref<HTMLElement | null>(null);
const props = defineProps<{ message: AgentMessage; disabled: boolean }>();
const emit = defineEmits<{ adopt: [messageId: string, ids: string[]] }>();
const cards = computed(() => (props.message.data?.cards ?? []) as GeneratedCard[]);
const adopted = computed(() => (props.message.data?.adoptedIds ?? []) as string[]);
const warnings = computed(() => (props.message.data?.warnings ?? []) as string[]);
const selected = ref(cards.value.filter(c => !adopted.value.includes(c.draftId)).map(c => c.draftId));
/** 勾选只改变待采纳集合，生成结果在用户确认前不会进入卡片库。 */
function toggle(id: string): void { selected.value = selected.value.includes(id) ? selected.value.filter(item => item !== id) : [...selected.value, id]; }
</script>
<template>
  <section class="rounded-xl border border-hairline bg-bg-side p-4" aria-label="Agent 生成的卡片草稿">
    <p class="text-[13px] font-medium">{{ cards.length }} 张卡片草稿 · {{ message.data?.path }}</p>
    <p v-for="(warning, i) in warnings" :key="i" class="mt-2 text-[12px] text-ink-3">{{ warning }}</p>
    <p v-if="!cards.length" class="mt-2 text-[12px]">材料中没有足够的可用考点，可以补充笔记后重试。</p>
    <div ref="draftsScroll" class="soft-scrollbar my-3 max-h-96 space-y-2 overflow-auto">
      <label v-for="card in cards" :key="card.draftId" class="flex items-start gap-3 rounded border border-hairline bg-bg-paper p-3 text-[12px]">
        <input type="checkbox" class="mt-1" :checked="selected.includes(card.draftId) || adopted.includes(card.draftId)" :disabled="disabled || adopted.includes(card.draftId)" @change="toggle(card.draftId)" />
        <span class="min-w-0 flex-1"><span class="block font-medium">{{ card.fields.front }}</span><span class="mt-1 block whitespace-pre-wrap leading-6 text-ink-2">{{ card.fields.back }}</span><span v-if="card.fields.detail" class="block text-ink-3">{{ card.fields.detail }}<PhoneticAudio v-if="isPhonetic(card.fields.detail)" :word="card.fields.back" /></span><span v-if="adopted.includes(card.draftId)" class="text-success">已处理</span></span>
      </label>
    </div>
    <VirtualScrollbar :target="draftsScroll" />
    <button v-if="cards.length" class="primary-btn" :disabled="disabled || !selected.some(id => !adopted.includes(id))" @click="emit('adopt', message.id, selected.filter(id => !adopted.includes(id)))">采纳选中卡片</button>
  </section>
</template>
