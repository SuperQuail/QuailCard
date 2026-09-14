<script setup lang="ts">
import { FilePlus2, FolderPlus, Pencil, Trash2 } from "@lucide/vue";
import type { ContextTarget } from "./useTreeContextMenu";

/**
 * 文件树右键菜单：只负责按目标类型展示菜单项并把意图交给 FileTree。
 * Teleport 到 body，避免被侧栏 overflow/堆叠上下文裁剪。
 */
defineProps<{ context: ContextTarget }>();

const emit = defineEmits<{
  "create-note": [parent: string | null];
  "create-folder": [parent: string | null];
  rename: [kind: "folder" | "note", key: string];
  remove: [kind: "folder" | "note", key: string];
  close: [];
}>();

/** 菜单项样式：删除项用危险色，其余用正文色。 */
const MENU_ITEM = "flex w-full items-center gap-2 rounded-md px-2.5 py-1.5 text-left text-[12px] hover:bg-bg-hover";
</script>

<template>
  <Teleport to="body">
    <div class="modal-panel fixed z-80 w-[150px] p-1" :style="{ left: `${context.x}px`, top: `${context.y}px` }">
      <template v-if="context.kind === 'folder'">
        <button type="button" :class="[MENU_ITEM, 'text-ink-2']" @click="emit('create-note', context.key)">
          <FilePlus2 :size="13" />新建笔记
        </button>
        <button type="button" :class="[MENU_ITEM, 'text-ink-2']" @click="emit('create-folder', context.key)">
          <FolderPlus :size="13" />新建文件夹
        </button>
        <button type="button" :class="[MENU_ITEM, 'text-ink-2']" @click="emit('rename', 'folder', context.key)">
          <Pencil :size="13" />重命名
        </button>
        <button type="button" :class="[MENU_ITEM, 'text-danger']" @click="emit('remove', 'folder', context.key)">
          <Trash2 :size="13" />删除
        </button>
      </template>
      <template v-else-if="context.kind === 'blank'">
        <button type="button" :class="[MENU_ITEM, 'text-ink-2']" @click="emit('create-note', null)">
          <FilePlus2 :size="13" />新建笔记
        </button>
        <button type="button" :class="[MENU_ITEM, 'text-ink-2']" @click="emit('create-folder', null)">
          <FolderPlus :size="13" />新建文件夹
        </button>
      </template>
      <template v-else>
        <button type="button" :class="[MENU_ITEM, 'text-ink-2']" @click="emit('rename', 'note', context.key)">
          <Pencil :size="13" />重命名
        </button>
        <button type="button" :class="[MENU_ITEM, 'text-danger']" @click="emit('remove', 'note', context.key)">
          <Trash2 :size="13" />删除
        </button>
      </template>
    </div>
    <div class="fixed inset-0 z-70" @mousedown="emit('close')" @contextmenu.prevent="emit('close')" />
  </Teleport>
</template>
