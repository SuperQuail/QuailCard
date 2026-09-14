<script setup lang="ts">
import { Brain, Moon, Sun } from "@lucide/vue";

defineProps<{
  vaultName: string;
  wordCount: number;
  dueCount: number;
  noteTitle: string;
  dark: boolean;
}>();

const emit = defineEmits<{ "toggle-theme": [] }>();
</script>

<template>
  <!-- 底部状态栏：极简、克制的元信息 -->
  <footer class="flex h-7 shrink-0 items-center justify-between border-t border-hairline bg-bg-side px-3 text-[10px] text-ink-3">
    <span class="truncate">{{ vaultName }}<span v-if="noteTitle" class="mx-1">/</span><span v-if="noteTitle" class="text-ink-2">{{ noteTitle }}</span></span>
    <span class="flex shrink-0 items-center gap-3">
      <span>{{ wordCount }} 字</span>
      <span class="flex items-center gap-1 text-accent-strong">
        <Brain :size="11" />今日待复习 {{ dueCount }}
      </span>
      <button type="button" class="flex items-center gap-1 transition hover:text-ink" title="切换主题" @click="emit('toggle-theme')">
        <template v-if="dark"><Moon :size="11" />深色</template>
        <template v-else><Sun :size="11" />浅色</template>
      </button>
    </span>
  </footer>
</template>
