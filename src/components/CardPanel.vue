<script setup lang="ts">
import { BookOpen, Brain, PanelRightClose, Pencil, Plus, Sparkles, Trash2, Type } from "@lucide/vue";
import type { NoteCard } from "../domain/types";

/**
 * 卡片列表：只负责"当前笔记有哪些卡片"的展示与操作上抛。
 * 大纲与反链已拆到右栏同级页签（NoteSidePanel），这里不再兼任第二职责。
 */
defineProps<{
  cards: NoteCard[];
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
</script>

<template>
  <div class="relative flex h-full flex-col">
    <!-- 面板头：卡片计数 + 收起 -->
    <header class="flex items-center justify-between border-b border-hairline py-2 pr-2 pl-3">
      <p class="flex items-center gap-1 text-[11px] font-medium text-ink-2">卡片 {{ cards.length }}</p>
      <button type="button" class="icon-btn" title="收起侧栏" @click="emit('collapse')">
        <PanelRightClose :size="15" :stroke-width="1.8" />
      </button>
    </header>

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
  </div>
</template>
