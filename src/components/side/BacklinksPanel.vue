<script setup lang="ts">
import type { Backlink } from "../../domain/types";

/**
 * 反链面板：列出正文里引用了当前笔记的其他笔记。
 *
 * 契约：只展示传入的反链数据，点击回传来源路径与行号（从 1 开始），
 * 面板自己不解析链接、不打开标签页。
 */
defineProps<{ backlinks: Backlink[] }>();

const emit = defineEmits<{ open: [path: string, line: number] }>();

/** 点击一条反链：把来源路径与行号交给宿主跳转。 */
function openLink(link: Backlink): void {
  emit("open", link.sourcePath, link.line);
}
</script>

<template>
  <div class="flex h-full min-h-0 flex-col text-[12px]">
    <h3 class="shrink-0 px-3 py-2 text-[11px] text-ink-3">反向链接（{{ backlinks.length }}）</h3>
    <ul class="soft-scrollbar min-h-0 flex-1 overflow-y-auto px-1 pb-2">
      <li v-for="link in backlinks" :key="link.sourcePath + ':' + link.line">
        <button
          type="button"
          class="block w-full rounded-md px-2 py-1.5 text-left transition hover:bg-bg-hover"
          :data-backlink="link.sourcePath + ':' + link.line"
          @click="openLink(link)"
        >
          <span class="flex items-baseline gap-2">
            <span class="min-w-0 flex-1 truncate text-ink">{{ link.sourceTitle }}</span>
            <span class="shrink-0 text-[10px] text-ink-3">L{{ link.line }}</span>
          </span>
          <span class="block truncate text-[11px] text-ink-3">{{ link.snippet }}</span>
        </button>
      </li>
      <li v-if="backlinks.length === 0" class="px-2 py-4 text-[11px] text-ink-3">还没有笔记引用它</li>
    </ul>
  </div>
</template>
