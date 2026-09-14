<script setup lang="ts">
import { BookOpen, PanelRightClose, PanelRightOpen, Pencil } from "@lucide/vue";

defineProps<{
  notePath?: string;
  panelOpen: boolean;
  reading?: boolean;
  editorError?: string;
  inline?: boolean;
}>();
const emit = defineEmits<{ "toggle-panel": []; "toggle-reading": [] }>();

</script>

<template>
  <!-- 标签存在时操作并入同一行；空工作区仍只显示紧凑的角落按钮。 -->
  <header class="note-toolbar z-20 flex flex-col items-end gap-2 text-[11px] text-ink-3" :class="inline ? 'relative' : 'absolute right-6 top-2 max-w-[calc(100%_-_48px)]'">
    <div class="note-actions flex shrink-0 items-center gap-1" role="group" aria-label="笔记操作">
      <button v-if="notePath" type="button" class="icon-btn" :aria-label="reading ? '切换到编辑模式' : '切换到阅读模式'" :title="reading ? '切换到编辑模式' : '切换到阅读模式'" @click="emit('toggle-reading')">
        <Pencil v-if="reading" :size="16" :stroke-width="1.8" aria-hidden="true" />
        <BookOpen v-else :size="16" :stroke-width="1.8" aria-hidden="true" />
      </button>
      <button type="button" class="icon-btn" :class="{ 'is-open': panelOpen }" :title="panelOpen ? '收起右侧栏（Alt+B）' : '打开右侧栏（Alt+B）'" :aria-label="panelOpen ? '收起右侧栏' : '打开右侧栏'" :aria-expanded="panelOpen" aria-controls="note-card-panel" @click="emit('toggle-panel')">
        <PanelRightClose v-if="panelOpen" :size="16" :stroke-width="1.8" aria-hidden="true" />
        <PanelRightOpen v-else :size="16" :stroke-width="1.8" aria-hidden="true" />
      </button>
    </div>
    <span v-if="editorError" role="alert" class="max-w-full truncate bg-bg-paper text-danger" :class="{ 'absolute right-0 top-full max-w-80': inline }" :title="editorError">{{ editorError }}</span>
  </header>
</template>

<style scoped>
/* 操作组只统一尺寸；悬停与键盘焦点沿用现有主题，展开状态仅强调图标颜色。 */
.note-actions .icon-btn { width: 28px; height: 28px; }
.note-actions .icon-btn.is-open { color: var(--qc-accent-strong); }

</style>
