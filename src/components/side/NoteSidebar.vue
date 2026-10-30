<script setup lang="ts">
import FileTree from "../FileTree.vue";
import SideTabs from "./SideTabs.vue";
import TagPanel from "./TagPanel.vue";
import type { NotePathChange } from "../../domain/notePaths";
import type { NoteSummary } from "../../domain/types";
import { leftSidebarTab } from "../../services/stores/uiStore";
import type { DeleteTarget } from "../../composables/useNoteActions";
import type { NoteMove } from "../../domain/notePaths";

/**
 * 左栏容器：「文件」与「标签」两个页签。
 *
 * 页签状态放在 uiStore，收起文件树后再次展开仍停在上次的栏位；
 * 本组件只做装配，所有业务动作一律上抛给壳层。
 */
defineProps<{
  notes: NoteSummary[];
  /** 完整文件夹列表：由笔记路径与已建空文件夹推导。 */
  folderNames: string[];
  activeNotePath: string | null;
  dueCount: number;
  pathChange: NotePathChange | null;
}>();

const emit = defineEmits<{
  "select-note": [path: string];
  "note-created": [folder: string, title: string];
  "folder-created": [path: string];
  "rename-note": [oldPath: string, newPath: string, done: (error?: string) => void];
  "delete-note": [path: string];
  "rename-folder": [oldPath: string, newPath: string, done: (error?: string) => void];
  "delete-folder": [path: string];
  "delete-selection": [items: DeleteTarget[]];
  "move-entries": [moves: NoteMove[]];
  "open-review": [];
}>();

/** 页签定义；新增栏位只需要在这里加一条并提供对应面板。 */
const tabs = [
  { id: "files", label: "文件" },
  { id: "tags", label: "标签" },
];

/** 只接受两个合法页签值，避免 store 被写入未定义状态。 */
function selectTab(id: string): void {
  if (id === "files" || id === "tags") {
    leftSidebarTab.value = id;
  }
}
</script>

<template>
  <div class="flex h-full flex-col">
    <SideTabs :tabs="tabs" :active="leftSidebarTab" @select="selectTab" />
    <div class="min-h-0 flex-1">
      <FileTree
        v-if="leftSidebarTab === 'files'"
        :notes="notes"
        :folder-names="folderNames"
        :active-note-path="activeNotePath"
        :due-count="dueCount"
        :path-change="pathChange"
        @select-note="emit('select-note', $event)"
        @note-created="(folder, title) => emit('note-created', folder, title)"
        @folder-created="emit('folder-created', $event)"
        @rename-note="(oldPath, newPath, done) => emit('rename-note', oldPath, newPath, done)"
        @delete-note="emit('delete-note', $event)"
        @rename-folder="(oldPath, newPath, done) => emit('rename-folder', oldPath, newPath, done)"
        @delete-folder="emit('delete-folder', $event)"
        @delete-selection="emit('delete-selection', $event)"
        @move-entries="emit('move-entries', $event)"
        @open-review="emit('open-review')"
      />
      <TagPanel v-else :notes="notes" @select-note="emit('select-note', $event)" />
    </div>
  </div>
</template>
