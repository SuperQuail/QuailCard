<script setup lang="ts">
import { computed, ref } from "vue";
import { buildTagTree, filterNotesByTag, normalizeTag, type TagNode } from "../../domain/tags";
import type { NoteSummary } from "../../domain/types";

/**
 * 标签面板：按标签树筛选笔记。
 *
 * 契约：只接收笔记摘要，所有导航动作通过 select-note 上抛；
 * 面板自身不读 store、不调后端，标签树与过滤都是 domain/tags 的纯函数结果。
 */
const props = defineProps<{ notes: NoteSummary[] }>();

const emit = defineEmits<{ "select-note": [path: string] }>();

/** 树的一行：节点加缩进层级；折叠时只省略子级，不重建树。 */
interface TagRow {
  node: TagNode;
  depth: number;
}

/** 顶部过滤输入：按完整标签路径包含匹配。 */
const query = ref("");
/** 折叠状态按完整路径记录，树数据刷新后同一标签仍保持展开。 */
const expanded = ref<Set<string>>(new Set());
/** 当前选中的标签（完整路径）；标签消失时自动视为未选中。 */
const selected = ref<string | null>(null);

/** 标签树：笔记摘要变化时重算，排序与计数都在 domain/tags 里完成。 */
const tree = computed(() => buildTagTree(props.notes));

/** 全部标签路径，用于判断选中项是否仍然存在。 */
const allPaths = computed(() => {
  const paths = new Set<string>();
  const walk = (nodes: TagNode[]): void => {
    for (const node of nodes) {
      paths.add(node.fullPath);
      walk(node.children);
    }
  };
  walk(tree.value);
  return paths;
});

/** 有效选中项：标签被删除后不再显示对应笔记。 */
const selectedTag = computed(() => (selected.value && allPaths.value.has(selected.value) ? selected.value : null));

/** 选中标签命中的笔记（含子标签）；未选中时为空。 */
const matchedNotes = computed(() =>
  selectedTag.value ? filterNotesByTag(props.notes, selectedTag.value) : []);

/** 子树里是否有标签命中关键字；过滤时用它保留祖先链。 */
function subtreeMatches(node: TagNode, keyword: string): boolean {
  return node.fullPath.toLowerCase().includes(keyword)
    || node.children.some((child) => subtreeMatches(child, keyword));
}

/**
 * 展开后的可见行：过滤时展开匹配项以便看到层级，
 * 不过滤时按折叠集合逐层展开；子节点不会被无条件铺平。
 */
const rows = computed<TagRow[]>(() => {
  const keyword = normalizeTag(query.value).toLowerCase();
  const visible: TagRow[] = [];
  const walk = (nodes: TagNode[], depth: number): void => {
    for (const node of nodes) {
      // 命中自身或任一后代时都显示本节点，保留从根到命中项的完整路径。
      const matched = !keyword || subtreeMatches(node, keyword);
      if (matched) visible.push({ node, depth });
      // 过滤时始终下探，否则匹配的深层标签会被父级折叠藏起来。
      const open = keyword || expanded.value.has(node.fullPath);
      if (open && node.children.length) walk(node.children, depth + 1);
    }
  };
  walk(tree.value, 0);
  return visible;
});

/** 点击标签：选中并切换折叠；再次点击同一标签只切换折叠。 */
function selectTag(node: TagNode): void {
  selected.value = node.fullPath;
  if (node.children.length === 0) return;
  const next = new Set(expanded.value);
  if (next.has(node.fullPath)) next.delete(node.fullPath);
  else next.add(node.fullPath);
  expanded.value = next;
}

/** 行内是否展开，供模板与 aria-expanded 共用。 */
function isExpanded(node: TagNode): boolean {
  return expanded.value.has(node.fullPath);
}

/** 按层级给出缩进：模板里用普通函数拼样式，避免在属性中塞模板字符串。 */
function rowIndent(depth: number): string {
  return `${8 + depth * 12}px`;
}

/** 点击笔记行：只上抛路径，不在这里打开标签页。 */
function openNote(path: string): void {
  emit("select-note", path);
}
</script>

<template>
  <div class="flex h-full min-h-0 flex-col text-[12px]">
    <div class="shrink-0 px-3 py-2">
      <input
        v-model="query"
        class="field-input w-full"
        type="search"
        placeholder="筛选标签"
        aria-label="筛选标签"
      />
    </div>
    <div class="soft-scrollbar min-h-0 flex-1 overflow-y-auto px-1 pb-2">
      <p v-if="rows.length === 0" class="px-2 py-4 text-[11px] text-ink-3">
        {{ tree.length === 0 ? "还没有标签" : "没有匹配的标签" }}
      </p>
      <button
        v-for="row in rows"
        :key="row.node.fullPath"
        type="button"
        class="flex w-full items-center gap-1 rounded-md py-1 pr-2 text-left transition hover:bg-bg-hover"
        :class="row.node.fullPath === selectedTag ? 'bg-bg-active text-ink' : 'text-ink-2'"
        :data-tag="row.node.fullPath"
        :aria-expanded="row.node.children.length ? isExpanded(row.node) : undefined"
        :style="{ paddingLeft: rowIndent(row.depth) }"
        @click="selectTag(row.node)"
      >
        <span class="w-3 shrink-0 text-[10px] text-ink-3">
          {{ row.node.children.length ? (isExpanded(row.node) ? "▾" : "▸") : "" }}
        </span>
        <span class="min-w-0 flex-1 truncate">{{ row.node.name }}</span>
        <span class="shrink-0 text-[11px] text-ink-3">{{ row.node.count }}</span>
      </button>
    </div>
    <section v-if="selectedTag" class="flex min-h-0 shrink-0 flex-col border-t border-hairline">
      <h3 class="px-3 py-1.5 text-[11px] text-ink-3">匹配笔记（{{ matchedNotes.length }}）</h3>
      <ul class="soft-scrollbar max-h-48 overflow-y-auto px-1 pb-2">
        <li v-for="note in matchedNotes" :key="note.path">
          <button
            type="button"
            class="block w-full truncate rounded-md px-2 py-1 text-left text-ink-2 transition hover:bg-bg-hover"
            :data-note-path="note.path"
            @click="openNote(note.path)"
          >
            {{ note.title }}
          </button>
        </li>
        <li v-if="matchedNotes.length === 0" class="px-2 py-2 text-[11px] text-ink-3">没有匹配笔记</li>
      </ul>
    </section>
  </div>
</template>
