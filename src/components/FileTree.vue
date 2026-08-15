<script setup lang="ts">
import { ChevronRight, FilePlus2, FileText, Folder, FolderOpen, FolderPlus, Pencil, Trash2 } from "@lucide/vue";
import { computed, ref } from "vue";
import type { NoteSummary } from "../domain/types";
import { buildTree, countNotes, findFolderNode, flattenRows, hasChildren, noteFolder, rowIndent, type TreeRow } from "./fileTree/treeModel";
import { useTreeContextMenu } from "./fileTree/useTreeContextMenu";
import { useTreeEditing } from "./fileTree/useTreeEditing";
import { useTreeSelection } from "./fileTree/useTreeSelection";

const props = defineProps<{
  notes: NoteSummary[];
  folderNames: string[];
  activeNotePath: string | null;
  dueCount: number;
}>();

const emit = defineEmits<{
  "select-note": [path: string];
  "note-created": [folder: string, title: string];
  "folder-created": [path: string];
  "rename-note": [oldPath: string, newPath: string];
  "delete-note": [path: string];
  "rename-folder": [oldPath: string, newPath: string];
  "delete-folder": [path: string];
  "open-review": [];
  "delete-selection": [items: Array<{ kind: "folder" | "note"; path: string }>];
}>();

const expanded = ref<Set<string>>(new Set(props.folderNames));
const tree = computed(() => buildTree(props.folderNames, props.notes));
const rows = computed(() => flattenRows(tree.value, expanded.value));

const { context, openFolderMenu, openNoteMenu, openBlankMenu, closeMenu } = useTreeContextMenu();
const { selection, selectRow, isSelected, buildDeleteItems, clearSelection } = useTreeSelection(rows);
const { creating, createValue, renaming, setCreateInput, startCreating, commitCreate, cancelCreate, startRenaming, commitRename, cancelRename } = useTreeEditing();

/** 展开或收起文件夹。 */
function toggleFolder(path: string): void {
  const next = new Set(expanded.value);
  if (next.has(path)) {
    next.delete(path);
  } else {
    next.add(path);
  }
  expanded.value = next;
}

/** 新建时展开父文件夹（编辑 composable 的展开钩子）。 */
function expandFolder(path: string): void {
  const next = new Set(expanded.value);
  next.add(path);
  expanded.value = next;
}

/** 当前笔记所在的文件夹（新建笔记的默认落点）。 */
function resolveActiveFolder(): string | null {
  return noteFolder(props.notes, props.activeNotePath);
}

/** 打开笔记并关闭菜单。 */
function selectNote(path: string): void {
  closeMenu();
  emit("select-note", path);
}

/** 进入新建模式（同时关闭菜单）。 */
function beginCreating(kind: "note" | "folder", parent: string | null): void {
  startCreating(kind, parent, expandFolder);
  closeMenu();
}

/** 进入重命名模式（同时关闭菜单）。 */
function beginRenaming(kind: "folder" | "note", key: string): void {
  if (kind === "folder") {
    const node = findFolderNode(tree.value, key);
    // 找不到节点时按路径推导显示名。
    startRenaming(kind, key, node ? node.name : key.split("/").pop() ?? key);
  } else {
    const note = props.notes.find((item) => item.path === key);
    startRenaming(kind, key, note?.title ?? "");
  }
  closeMenu();
}

/** 提交新建：转换为对应 emit。 */
function submitCreate(kind: "note" | "folder", parent: string | null, value: string): void {
  if (kind === "note") {
    emit("note-created", parent ?? "", value);
  } else {
    emit("folder-created", parent ? `${parent}/${value}` : value);
  }
}

/** 提交重命名：转换为对应 emit。 */
function submitRename(kind: "folder" | "note", oldPath: string, newPath: string): void {
  if (kind === "folder") {
    emit("rename-folder", oldPath, newPath);
  } else {
    emit("rename-note", oldPath, newPath);
  }
}

/** 删除菜单目标条目。 */
function removeTarget(): void {
  if (!context.value) {
    return;
  }
  if (context.value.kind === "folder") {
    emit("delete-folder", context.value.key);
  } else {
    emit("delete-note", context.value.key);
  }
  closeMenu();
}

/** 文件夹行点击：更新选择并展开/收起。 */
function onFolderRowClick(row: Extract<TreeRow, { kind: "folder" }>, event: MouseEvent): void {
  selectRow(row, event);
  toggleFolder(row.node.path);
}

/** 笔记行点击：更新选择并打开。 */
function onNoteRowClick(row: Extract<TreeRow, { kind: "note" }>, event: MouseEvent): void {
  selectRow(row, event);
  selectNote(row.note.path);
}

/** 树内键盘：Delete/Backspace 批量删除，Esc 清除选择。 */
function handleTreeKeydown(event: KeyboardEvent): void {
  const target = event.target as HTMLElement | null;
  if (target instanceof HTMLInputElement || target instanceof HTMLTextAreaElement) {
    return;
  }
  if (creating.value || renaming.value) {
    return;
  }
  if (event.key === "Delete" || event.key === "Backspace") {
    if (selection.value.size === 0) {
      return;
    }
    event.preventDefault();
    const items = buildDeleteItems();
    if (items.length > 0) {
      emit("delete-selection", items);
    }
    clearSelection();
    return;
  }
  if (event.key === "Escape") {
    clearSelection();
  }
}
</script>

<template>
  <div class="flex h-full flex-col" @click.self="closeMenu" @contextmenu.prevent="openBlankMenu">
    <!-- 侧栏头：Vault 名称 + 悬停显示的新建按钮（VS Code 式） -->
    <header class="group flex items-center gap-0.5 px-3 pt-2.5 pb-1.5">
      <div class="min-w-0 flex-1">
        <p class="truncate text-[13px] font-semibold">笔记</p>
        <p class="text-[10px] text-ink-3">{{ notes.length }} 篇笔记</p>
      </div>
      <button type="button" class="icon-btn opacity-0 transition-opacity duration-100 group-hover:opacity-100" title="新建笔记" @click="beginCreating('note', resolveActiveFolder())">
        <FilePlus2 :size="15" :stroke-width="1.8" />
      </button>
      <button type="button" class="icon-btn opacity-0 transition-opacity duration-100 group-hover:opacity-100" title="新建文件夹" @click="beginCreating('folder', null)">
        <FolderPlus :size="15" :stroke-width="1.8" />
      </button>
    </header>

    <!-- 文件树 -->
    <div class="soft-scrollbar min-h-0 flex-1 overflow-y-auto px-2 py-1 outline-none" tabindex="-1" @keydown="handleTreeKeydown">
      <input
        v-if="creating && creating.parent === null"
        :ref="setCreateInput"
        v-model="createValue"
        class="tree-input my-0.5 ml-2"
        :placeholder="creating.kind === 'note' ? '笔记名称' : '文件夹名称'"
        @keyup.enter="commitCreate(submitCreate)"
        @keyup.esc="cancelCreate"
        @blur="cancelCreate"
      />

      <template v-for="row in rows" :key="row.kind === 'folder' ? `f-${row.node.path}` : `n-${row.note.path}`">
        <!-- 文件夹行 -->
        <template v-if="row.kind === 'folder'">
          <input
            v-if="renaming && renaming.kind === 'folder' && renaming.key === row.node.path"
            v-model="renaming.value"
            class="tree-input my-px"
            :style="{ marginLeft: rowIndent(row.depth) }"
            autofocus
            @keyup.enter="commitRename(submitRename)"
            @keyup.esc="cancelRename"
            @blur="commitRename(submitRename)"
          />
          <button
            v-else
            type="button"
            class="tree-row relative flex w-full items-center gap-0.5 rounded-md pr-2 text-left text-[12px] font-medium transition-colors duration-75"
            :class="[hasChildren(row.node) ? 'text-ink-2' : 'text-ink-3', 'hover:bg-bg-hover', isSelected(row) ? 'is-selected' : '']"
            :style="{ paddingLeft: rowIndent(row.depth) }"
            @click="onFolderRowClick(row, $event)"
            @contextmenu.stop.prevent="openFolderMenu(row.node.path, $event)"
          >
            <span v-for="guideIndex in row.depth" :key="guideIndex" class="tree-guide" :style="{ left: `${15 + (guideIndex - 1) * 16}px` }" />
            <span class="flex w-4 shrink-0 items-center justify-center">
              <ChevronRight v-if="hasChildren(row.node)" :size="12" class="shrink-0 text-ink-3 transition-transform duration-100" :class="expanded.has(row.node.path) ? 'rotate-90' : ''" />
            </span>
            <FolderOpen v-if="expanded.has(row.node.path) && hasChildren(row.node)" :size="14" :stroke-width="1.6" class="shrink-0 text-ink-3" />
            <Folder v-else :size="14" :stroke-width="1.6" class="shrink-0 text-ink-3" />
            <span class="truncate">{{ row.node.name }}</span>
            <span v-if="!expanded.has(row.node.path) && countNotes(row.node) > 0" class="ml-auto shrink-0 rounded-full bg-marker px-1.5 text-[9px] font-semibold text-accent-strong">{{ countNotes(row.node) }}</span>
          </button>
          <input
            v-if="creating && creating.parent === row.node.path && expanded.has(row.node.path)"
            :ref="setCreateInput"
            v-model="createValue"
            class="tree-input my-px"
            :style="{ marginLeft: rowIndent(row.depth + 1) }"
            :placeholder="creating.kind === 'note' ? '笔记名称' : '文件夹名称'"
            @keyup.enter="commitCreate(submitCreate)"
            @keyup.esc="cancelCreate"
            @blur="cancelCreate"
          />
        </template>

        <!-- 笔记行 -->
        <template v-else>
          <input
            v-if="renaming && renaming.kind === 'note' && renaming.key === row.note.path"
            v-model="renaming.value"
            class="tree-input my-px"
            :style="{ marginLeft: rowIndent(row.depth) }"
            autofocus
            @keyup.enter="commitRename(submitRename)"
            @keyup.esc="cancelRename"
            @blur="commitRename(submitRename)"
          />
          <button
            v-else
            type="button"
            class="tree-row relative flex w-full items-center gap-0.5 rounded-md pr-2 text-left text-[12px] transition-colors duration-75"
            :class="[activeNotePath === row.note.path ? 'is-active bg-bg-active font-medium text-accent-strong' : 'text-ink-2', 'hover:bg-bg-hover', isSelected(row) ? 'is-selected' : '']"
            :style="{ paddingLeft: rowIndent(row.depth) }"
            @click="onNoteRowClick(row, $event)"
            @contextmenu.stop.prevent="openNoteMenu(row.note.path, $event)"
          >
            <span v-for="guideIndex in row.depth" :key="guideIndex" class="tree-guide" :style="{ left: `${15 + (guideIndex - 1) * 16}px` }" />
            <span class="w-4 shrink-0" />
            <FileText :size="13" :stroke-width="1.6" class="shrink-0" />
            <span class="truncate">{{ row.note.title }}</span>
            <span v-if="row.note.cardCount > 0" class="ml-auto shrink-0 rounded-full bg-marker px-1.5 text-[9px] font-semibold text-accent-strong">{{ row.note.cardCount }}</span>
          </button>
        </template>
      </template>
    </div>

    <!-- 底部：今日到期 -->
    <footer class="border-t border-hairline px-3 py-1.5">
      <button type="button" class="flex w-full items-center justify-between rounded-md px-1.5 py-1 text-[11px] text-ink-2 transition hover:bg-bg-hover" @click="emit('open-review')">
        <span>今日待复习</span>
        <span class="font-semibold" :class="dueCount > 0 ? 'text-accent-strong' : 'text-ink-3'">{{ dueCount }}</span>
      </button>
    </footer>

    <!-- 右键菜单：Teleport 到 body，避免被侧栏 overflow/堆叠上下文裁剪 -->
    <Teleport to="body">
      <div
        v-if="context"
        class="modal-panel fixed z-80 w-[150px] p-1"
        :style="{ left: `${context.x}px`, top: `${context.y}px` }"
      >
      <template v-if="context.kind === 'folder'">
        <button type="button" class="flex w-full items-center gap-2 rounded-md px-2.5 py-1.5 text-left text-[12px] text-ink-2 hover:bg-bg-hover" @click="beginCreating('note', context.key)">
          <FilePlus2 :size="13" />新建笔记
        </button>
        <button type="button" class="flex w-full items-center gap-2 rounded-md px-2.5 py-1.5 text-left text-[12px] text-ink-2 hover:bg-bg-hover" @click="beginCreating('folder', context.key)">
          <FolderPlus :size="13" />新建文件夹
        </button>
        <button type="button" class="flex w-full items-center gap-2 rounded-md px-2.5 py-1.5 text-left text-[12px] text-ink-2 hover:bg-bg-hover" @click="beginRenaming('folder', context.key)">
          <Pencil :size="13" />重命名
        </button>
        <button type="button" class="flex w-full items-center gap-2 rounded-md px-2.5 py-1.5 text-left text-[12px] text-danger hover:bg-bg-hover" @click="removeTarget">
          <Trash2 :size="13" />删除
        </button>
      </template>
      <template v-else-if="context.kind === 'blank'">
        <button type="button" class="flex w-full items-center gap-2 rounded-md px-2.5 py-1.5 text-left text-[12px] text-ink-2 hover:bg-bg-hover" @click="beginCreating('note', resolveActiveFolder())">
          <FilePlus2 :size="13" />新建笔记
        </button>
        <button type="button" class="flex w-full items-center gap-2 rounded-md px-2.5 py-1.5 text-left text-[12px] text-ink-2 hover:bg-bg-hover" @click="beginCreating('folder', null)">
          <FolderPlus :size="13" />新建文件夹
        </button>
      </template>
      <template v-else>
        <button type="button" class="flex w-full items-center gap-2 rounded-md px-2.5 py-1.5 text-left text-[12px] text-ink-2 hover:bg-bg-hover" @click="beginRenaming('note', context.key)">
          <Pencil :size="13" />重命名
        </button>
        <button type="button" class="flex w-full items-center gap-2 rounded-md px-2.5 py-1.5 text-left text-[12px] text-danger hover:bg-bg-hover" @click="removeTarget">
          <Trash2 :size="13" />删除
        </button>
      </template>
    </div>
      <div v-if="context" class="fixed inset-0 z-70" @mousedown="closeMenu" @contextmenu.prevent="closeMenu" />
    </Teleport>
  </div>
</template>
