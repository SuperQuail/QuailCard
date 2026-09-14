<script setup lang="ts">
import { Brain, ChevronRight, FilePlus2, FileText, Folder, FolderOpen, FolderPlus, Search, X } from "@lucide/vue";
import { computed, nextTick, ref, watch } from "vue";
import { parentPath, remapNotePath, type NoteMoveItem, type NotePathChange } from "../domain/notePaths";
import type { NoteSummary } from "../domain/types";
import FileTreeMenu from "./fileTree/FileTreeMenu.vue";
import { buildTree, countNotes, findFolderNode, flattenRows, hasChildren, noteFolder, rowIndent, rowKey, type TreeRow } from "./fileTree/treeModel";
import { useTreeContextMenu } from "./fileTree/useTreeContextMenu";
import { useTreeDrag } from "./fileTree/useTreeDrag";
import { useTreeEditing } from "./fileTree/useTreeEditing";
import { useTreeSelection } from "./fileTree/useTreeSelection";
import VirtualScrollbar from "./VirtualScrollbar.vue";

/** 文件树滚动容器：浮层滚动条与它同级，按它的几何绘制。 */
const treeScroll = ref<HTMLElement | null>(null);

const props = defineProps<{
  notes: NoteSummary[];
  folderNames: string[];
  activeNotePath: string | null;
  dueCount: number;
  pathChange?: NotePathChange | null;
}>();

const emit = defineEmits<{
  "select-note": [path: string];
  "note-created": [folder: string, title: string];
  "folder-created": [path: string];
  "rename-note": [oldPath: string, newPath: string, done: (error?: string) => void];
  "delete-note": [path: string];
  "rename-folder": [oldPath: string, newPath: string, done: (error?: string) => void];
  "delete-folder": [path: string];
  "open-review": [];
  "delete-selection": [items: Array<{ kind: "folder" | "note"; path: string }>];
  "move-entries": [moves: Array<{ kind: "folder" | "note"; from: string; to: string }>];
}>();

const expanded = ref<Set<string>>(new Set(props.folderNames));
const tree = computed(() => buildTree(props.folderNames, props.notes));
const searchQuery = ref("");
const searchInput = ref<HTMLInputElement | null>(null);
const searchTerm = computed(() => searchQuery.value.trim().toLocaleLowerCase());
/** 筛选时列出命中笔记，不受目录折叠影响，也不改变原来的展开状态。 */
const matchingNotes = computed(() => props.notes.filter((note) =>
  note.title.toLocaleLowerCase().includes(searchTerm.value) || note.path.toLocaleLowerCase().includes(searchTerm.value)));
const rows = computed<TreeRow[]>(() => searchTerm.value
  ? matchingNotes.value.map((note) => ({ kind: "note", note, depth: 0 }))
  : flattenRows(tree.value, expanded.value));

const { context, openFolderMenu, openNoteMenu, openBlankMenu, closeMenu } = useTreeContextMenu();
const { selection, selectionAnchor, selectRow, isSelected, buildDeleteItems, clearSelection } = useTreeSelection(rows);
const { creating, createValue, renaming, renameBusy, renameError, setCreateInput, startCreating, commitCreate, cancelCreate, startRenaming, commitRename, cancelRename } = useTreeEditing();

/** 切换筛选条件时清理旧选择和菜单，避免误操作已隐藏的笔记。 */
watch(searchQuery, () => { clearSelection(); closeMenu(); });

/** 清空后仍可继续输入，不把焦点留在消失的清除按钮上。 */
function clearSearch(): void {
  searchQuery.value = "";
  searchInput.value?.focus();
}

/** 回车打开首个匹配项；中文输入法确认候选时不触发笔记跳转。 */
function handleSearchKeydown(event: KeyboardEvent): void {
  if (event.isComposing || event.keyCode === 229) return;
  if (event.key === "Escape") {
    event.preventDefault(); event.stopPropagation(); clearSearch();
  } else if (event.key === "Enter" && searchTerm.value) {
    event.preventDefault();
    const note = matchingNotes.value[0];
    if (note) selectNote(note.path);
  }
}

/** 后端确认改名后同步树的路径身份，保留展开和多选。 */
watch(() => props.pathChange, (change) => {
  if (!change) return;
  const remap = (path: string): string => remapNotePath(path, change.oldPath, change.newPath);
  expanded.value = new Set([...expanded.value].map(remap));
  selection.value = new Set([...selection.value].map(remap));
  if (selectionAnchor.value) selectionAnchor.value = remap(selectionAnchor.value);
}, { flush: "sync" });

/** 动态输入框显式聚焦并选中名称，避免 autofocus 依赖窗口焦点变化。 */
let renameInputElement: HTMLInputElement | null = null;
/** 仅首次挂载选中名称，输入更新不能重复 select 吞掉已输入字符。 */
function focusRename(element: unknown): void {
  if (element === null) renameInputElement = null;
  if (element instanceof HTMLInputElement && element !== renameInputElement) {
    renameInputElement = element;
    void nextTick(() => { element.focus(); element.select(); });
  }
}

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

/** 拖拽起手：未选中的行先单选，已选中的行整组移动（与 VS Code 一致）。 */
function resolveDragItems(row: TreeRow): NoteMoveItem[] {
  if (!selection.value.has(rowKey(row))) {
    selectRow(row, { shiftKey: false, ctrlKey: false, metaKey: false });
  }
  const group = rows.value.filter((candidate) => selection.value.has(rowKey(candidate)));
  return (group.length > 0 ? group : [row]).map((candidate) => ({
    kind: candidate.kind === "folder" ? "folder" : "note",
    path: rowKey(candidate),
  }));
}

const { drag, ghost, label: dragLabel, isDragging, isDropTarget, onPointerDown, guardClick } = useTreeDrag({
  resolveItems: resolveDragItems,
  expand: expandFolder,
  isExpanded: (path) => expanded.value.has(path),
  onDrop: (moves) => emit("move-entries", moves),
  container: () => treeScroll.value,
});

/** 拖拽提示：显示落点目录与本次移动的条目数。 */
const dragHint = computed(() => {
  const session = drag.value;
  if (!session || session.folder === null) return "";
  return `松开后移动 ${session.count} 项到${session.folder ? `「${session.folder.split("/").pop()}」` : "根目录"}`;
});

/** 打开笔记并关闭菜单。 */
function selectNote(path: string): void {
  closeMenu();
  emit("select-note", path);
}

/** 进入新建模式（同时关闭菜单）。 */
function beginCreating(kind: "note" | "folder", parent: string | null): void {
  searchQuery.value = "";
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
function submitRename(kind: "folder" | "note", oldPath: string, newPath: string): Promise<void> {
  return new Promise((resolve, reject) => {
    const done = (error?: string): void => { if (error) reject(new Error(error)); else resolve(); };
    if (kind === "folder") emit("rename-folder", oldPath, newPath, done);
    else emit("rename-note", oldPath, newPath, done);
  });
}

/** 删除菜单目标条目。 */
function removeEntry(kind: "folder" | "note", key: string): void {
  if (kind === "folder") {
    emit("delete-folder", key);
  } else {
    emit("delete-note", key);
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
  <div class="file-tree relative flex h-full min-h-0 flex-col" :class="drag ? 'is-dragging' : ''" @click.self="closeMenu" @contextmenu.prevent="openBlankMenu">
    <!-- 侧栏头：标题与数量下提供搜索，新建入口始终可见、可键盘访问。 -->
    <header class="flex shrink-0 items-center gap-0.5 px-3 pt-3 pb-2">
      <div class="min-w-0 flex-1">
        <p class="truncate text-[13px] font-semibold">笔记</p>
        <p class="text-[10px] text-ink-3">{{ notes.length }} 篇笔记</p>
      </div>
      <button type="button" class="icon-btn shrink-0" title="新建笔记" @click="beginCreating('note', resolveActiveFolder())">
        <FilePlus2 :size="15" :stroke-width="1.8" />
      </button>
      <button type="button" class="icon-btn shrink-0" title="新建文件夹" @click="beginCreating('folder', null)">
        <FolderPlus :size="15" :stroke-width="1.8" />
      </button>
    </header>
    <div class="shrink-0 px-3 pb-2">
      <div class="tree-search flex h-8 w-full items-center gap-2 rounded-md border border-hairline bg-bg-paper px-2 text-[11px] text-ink-3 focus-within:border-accent">
        <Search :size="14" :stroke-width="1.8" class="shrink-0" aria-hidden="true" />
        <input ref="searchInput" v-model="searchQuery" type="search" aria-label="搜索笔记" title="按标题或路径筛选笔记" placeholder="搜索笔记…" autocomplete="off" class="tree-search-input h-full min-w-0 flex-1 bg-transparent text-ink placeholder:text-ink-3" @keydown="handleSearchKeydown" />
        <button v-if="searchQuery" type="button" class="flex shrink-0 items-center justify-center" aria-label="清空搜索" @click="clearSearch"><X :size="14" :stroke-width="1.8" /></button>
      </div>
    </div>

    <p v-if="searchTerm" role="status" class="shrink-0 px-3 pb-1 text-[10px] text-ink-3">{{ matchingNotes.length ? `找到 ${matchingNotes.length} 篇笔记` : '没有匹配的笔记' }}</p>

    <!-- 文件树 -->
    <p v-if="renameError" role="alert" class="px-3 py-1 text-[11px] text-danger">{{ renameError }}</p>
    <div ref="treeScroll" class="soft-scrollbar min-h-0 flex-1 overflow-y-auto px-2 py-1 outline-none" tabindex="-1" @keydown="handleTreeKeydown" @click.capture="guardClick" @dragstart.prevent>
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
            :ref="focusRename"
            v-model="renaming.value"
            :readonly="renameBusy"
            class="tree-input my-px"
            :style="{ marginLeft: rowIndent(row.depth) }"
            autofocus
            @keyup.enter="commitRename(submitRename)"
            @keyup.esc="cancelRename"
            @blur="!renameError && commitRename(submitRename)"
          />
          <button
            v-else
            type="button"
            class="tree-row relative flex w-full items-center gap-0.5 rounded-md pr-2 text-left text-[12px] font-medium transition-colors duration-75"
            :class="[hasChildren(row.node) ? 'text-ink-2' : 'text-ink-3', 'hover:bg-bg-hover', isSelected(row) ? 'is-selected' : '', isDragging(row) ? 'is-dragging' : '', isDropTarget(row) ? 'is-drop-target' : '']"
            :style="{ paddingLeft: rowIndent(row.depth) }"
            :data-tree-key="row.node.path"
            :data-drop-folder="row.node.path"
            @click="onFolderRowClick(row, $event)"
            @contextmenu.stop.prevent="openFolderMenu(row.node.path, $event)"
            @pointerdown="onPointerDown(row, $event)"
          >
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
            :ref="focusRename"
            v-model="renaming.value"
            :readonly="renameBusy"
            class="tree-input my-px"
            :style="{ marginLeft: rowIndent(row.depth) }"
            autofocus
            @keyup.enter="commitRename(submitRename)"
            @keyup.esc="cancelRename"
            @blur="!renameError && commitRename(submitRename)"
          />
          <button
            v-else
            type="button"
            class="tree-row relative flex w-full items-center gap-0.5 rounded-md pr-2 text-left text-[12px] transition-colors duration-75"
            :class="[activeNotePath === row.note.path ? 'is-active bg-bg-active font-medium text-accent-strong' : 'text-ink-2', 'hover:bg-bg-hover', isSelected(row) ? 'is-selected' : '', isDragging(row) ? 'is-dragging' : '', isDropTarget(row) ? 'is-drop-target' : '']"
            :style="{ paddingLeft: rowIndent(row.depth) }"
            :title="row.note.path"
            :data-search-result="Boolean(searchTerm)"
            :data-tree-key="row.note.path"
            :data-drop-folder="parentPath(row.note.path)"
            @click="onNoteRowClick(row, $event)"
            @contextmenu.stop.prevent="openNoteMenu(row.note.path, $event)"
            @pointerdown="onPointerDown(row, $event)"
          >
            <span v-if="!searchTerm" class="w-4 shrink-0" />
            <FileText :size="13" :stroke-width="1.6" class="shrink-0" />
            <span class="min-w-0 flex-1">
              <span class="block truncate">{{ row.note.title }}</span>
              <span v-if="searchTerm" class="block truncate text-[10px] font-normal text-ink-3">{{ row.note.path }}</span>
            </span>
            <span v-if="row.note.cardCount > 0" class="ml-auto shrink-0 rounded-full bg-marker px-1.5 text-[9px] font-semibold text-accent-strong">{{ row.note.cardCount }}</span>
          </button>
        </template>
      </template>
    </div>
    <VirtualScrollbar :target="treeScroll" />

    <!-- 底部：今日到期 -->
    <footer class="mt-auto shrink-0 border-t border-hairline px-3 py-2">
      <button type="button" class="tree-review flex min-h-8 w-full items-center gap-2 rounded-md px-1.5 py-1 text-[11px] text-ink-2 transition hover:bg-bg-hover" @click="emit('open-review')">
        <Brain :size="15" :stroke-width="1.8" class="shrink-0" aria-hidden="true" />
        <span class="flex-1 text-left">今日待复习</span>
        <span class="font-semibold" :class="dueCount > 0 ? 'text-accent-strong' : 'text-ink-3'">{{ dueCount }}</span>
      </button>
    </footer>

    <!-- 右键菜单：菜单项动作回到本组件的编辑状态机 -->
    <FileTreeMenu
      v-if="context"
      :context="context"
      @create-note="(parent) => beginCreating('note', parent ?? resolveActiveFolder())"
      @create-folder="(parent) => beginCreating('folder', parent)"
      @rename="beginRenaming"
      @remove="removeEntry"
      @close="closeMenu"
    />

    <!-- 拖拽提示：浮在树底，不占布局高度，避免拖动时行位置跳动 -->
    <p v-if="dragHint" role="status" class="tree-drag-hint">{{ dragHint }}</p>

    <!-- 拖拽幽灵：跟随指针，提示移动什么，不参与命中判定 -->
    <Teleport to="body">
      <div v-if="drag && ghost" class="tree-drag-ghost" :style="{ left: `${ghost.x}px`, top: `${ghost.y}px` }">
        <FileText :size="12" :stroke-width="1.6" />{{ dragLabel }}
      </div>
    </Teleport>
  </div>
</template>

<style scoped>
.tree-row {
  height: 32px;
  gap: 4px;
}

.tree-row[data-search-result="true"] {
  height: 42px;
}

.tree-search-input:focus-visible {
  outline: none;
}

.tree-search-input::-webkit-search-cancel-button {
  appearance: none;
}

.tree-input {
  height: 30px;
}

.file-tree button:focus-visible {
  outline: 1px solid var(--qc-accent);
  outline-offset: -1px;
}

/* 拖拽期间整棵树不接受文字选择，光标提示正在拖动。 */
.file-tree.is-dragging,
.file-tree.is-dragging .tree-row {
  cursor: grabbing;
  user-select: none;
}

/* 拖拽提示条：贴在树底部，拖动过程中不改变任何行的位置。 */
.tree-drag-hint {
  position: absolute;
  right: 10px;
  bottom: 52px;
  left: 10px;
  z-index: 5;
  padding: 3px 8px;
  border: 1px solid var(--qc-accent);
  border-radius: 4px;
  background: var(--qc-bg-paper);
  color: var(--qc-accent-strong);
  font-size: 10px;
  text-align: center;
  pointer-events: none;
}

/* 拖拽幽灵：贴指针右下角，绝不挡住落点命中。 */
.tree-drag-ghost {
  position: fixed;
  z-index: 90;
  display: flex;
  align-items: center;
  gap: 4px;
  max-width: 220px;
  padding: 2px 8px;
  border: 1px solid var(--qc-accent);
  border-radius: 4px;
  background: var(--qc-bg-paper);
  color: var(--qc-accent-strong);
  font-size: 11px;
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
  pointer-events: none;
  transform: translate(10px, 12px);
}
</style>
