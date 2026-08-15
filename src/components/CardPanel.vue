<script setup lang="ts">
import { BookOpen, Brain, FileText, ListTree, PanelRightClose, Pencil, Plus, Sparkles, Trash2, Type } from "@lucide/vue";
import { computed, ref } from "vue";
import type { NoteCard } from "../domain/types";
import type { NoteBlock } from "../markdown";

const props = defineProps<{
  notePath: string;
  cards: NoteCard[];
  blocks: NoteBlock[];
  activeCardId: string | null;
}>();

const emit = defineEmits<{
  "edit-card": [cardId: string];
  "delete-card": [cardId: string];
  "add-card": [];
  "open-ai-split": [];
  "start-review": [];
  "collapse": [];
}>();

const activeTab = ref<"cards" | "outline">("cards");

/** 提取正文标题作为大纲。 */
function resolveHeadings(): Array<{ index: number; text: string; level: number }> {
  const result: Array<{ index: number; text: string; level: number }> = [];
  for (const [index, block] of props.blocks.entries()) {
    if (block.type === "h1") {
      result.push({ index, text: block.text, level: 1 });
    } else if (block.type === "h2") {
      result.push({ index, text: block.text, level: 2 });
    } else if (block.type === "h3") {
      result.push({ index, text: block.text, level: 3 });
    }
  }
  return result;
}

/** 返回卡片类型的展示标签。 */
function kindLabel(kind: string): string {
  if (kind === "vocabulary") {
    return "单词";
  }
  return "问答";
}

/** 返回卡片类型的图标组件。 */
function kindIcon(kind: string) {
  if (kind === "vocabulary") {
    return Type;
  }
  return BookOpen;
}

/** 点击大纲标题时滚动正文到对应位置。 */
function jumpToHeading(index: number): void {
  document.getElementById(`heading-${index}`)?.scrollIntoView({ behavior: "smooth", block: "start" });
}

const headings = computed(resolveHeadings);
</script>

<template>
  <div class="flex h-full flex-col">
    <!-- 面板头：标签页切换 + 收起 -->
    <header class="flex items-center justify-between border-b border-hairline py-2 pr-2 pl-3">
      <div class="flex items-center gap-0.5">
        <button
          type="button"
          class="ghost-btn !text-[11px]"
          :class="{ '!text-accent-strong': activeTab === 'cards' }"
          @click="activeTab = 'cards'"
        >
          <FileText :size="13" />卡片 {{ cards.length }}
        </button>
        <button
          type="button"
          class="ghost-btn !text-[11px]"
          :class="{ '!text-accent-strong': activeTab === 'outline' }"
          @click="activeTab = 'outline'"
        >
          <ListTree :size="13" />大纲
        </button>
      </div>
      <button type="button" class="icon-btn" title="收起卡片面板" @click="emit('collapse')">
        <PanelRightClose :size="15" :stroke-width="1.8" />
      </button>
    </header>

    <!-- 大纲视图 -->
    <div v-if="activeTab === 'outline'" class="soft-scrollbar min-h-0 flex-1 overflow-y-auto px-3 py-2">
      <button
        v-for="heading in headings"
        :key="heading.index"
        type="button"
        class="block w-full truncate rounded-md px-2 py-1 text-left text-[12px] text-ink-2 transition hover:bg-bg-hover"
        :class="heading.level === 1 ? 'font-semibold text-ink' : heading.level === 2 ? 'pl-4' : 'pl-7'"
        @click="jumpToHeading(heading.index)"
      >
        {{ heading.text }}
      </button>
      <p v-if="headings.length === 0" class="px-2 py-4 text-[11px] text-ink-3">没有标题</p>
    </div>

    <!-- 卡片视图 -->
    <template v-else>
      <div class="soft-scrollbar min-h-0 flex-1 overflow-y-auto px-3 py-2">
        <div v-if="cards.length === 0" class="flex flex-col items-center px-4 py-10 text-center">
          <Sparkles :size="20" class="text-ink-3" />
          <p class="mt-3 text-[12px] font-medium">还没有卡片</p>
          <button type="button" class="primary-btn mt-4" @click="emit('open-ai-split')">
            <Sparkles :size="14" />AI 拆卡
          </button>
        </div>

        <ul v-else class="space-y-1.5">
          <li
            v-for="card in cards"
            :key="card.id"
            class="rounded-lg border transition"
            :class="activeCardId === card.id ? 'border-accent/60 bg-bg-active/50' : 'border-hairline bg-bg-paper hover:border-accent/40'"
          >
            <div class="px-3 pt-2.5 pb-2">
              <p class="line-clamp-2 text-[12px] leading-5 font-medium">{{ card.front }}</p>
              <p class="mt-1 line-clamp-2 text-[11px] leading-4 text-ink-3">{{ card.back }}</p>
              <div class="mt-2 flex items-center justify-between">
                <span class="flex items-center gap-1 text-[10px] text-ink-3">
                  <component :is="kindIcon(card.kind)" :size="11" />{{ kindLabel(card.kind) }}
                  <span v-if="card.schedulerPhase !== 'review' && card.intervalDays === 0" class="ml-1 rounded-full bg-marker px-1.5 py-px font-semibold text-accent-strong">今日</span>
                </span>
                <span class="flex items-center gap-0.5">
                  <button type="button" class="icon-btn !size-6" title="编辑卡片" @click="emit('edit-card', card.id)">
                    <Pencil :size="12" />
                  </button>
                  <button type="button" class="icon-btn !size-6 hover:!text-danger" title="删除卡片" @click="emit('delete-card', card.id)">
                    <Trash2 :size="12" />
                  </button>
                </span>
              </div>
            </div>
          </li>
        </ul>
      </div>

      <!-- 面板底部操作 -->
      <footer class="space-y-1.5 border-t border-hairline p-3">
        <button type="button" class="primary-btn w-full" :disabled="cards.length === 0" @click="emit('start-review')">
          <Brain :size="14" />复习这篇笔记
        </button>
        <div class="grid grid-cols-2 gap-1.5">
          <button type="button" class="ghost-btn justify-center border border-hairline" @click="emit('open-ai-split')">
            <Sparkles :size="13" />AI 拆卡
          </button>
          <button type="button" class="ghost-btn justify-center border border-hairline" @click="emit('add-card')">
            <Plus :size="13" />手动添加
          </button>
        </div>
      </footer>
    </template>
  </div>
</template>
