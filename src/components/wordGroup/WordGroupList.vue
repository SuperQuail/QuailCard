<script setup lang="ts">
import { computed, ref } from "vue";
import { ListFilter, Plus, Search } from "@lucide/vue";
import { isEntryComplete, type WordEntry } from "../../domain/wordGroup";

/**
 * 单词组左侧列表：组内导航、完成度与过滤。
 *
 * 契约：只负责展示与上抛选中页，不持有页面数据、不序列化；过滤条件属于本组件的
 * 展示状态，翻页与否仍由编辑器决定。「添加单词」放在列表底部，因为新页就追加在列表末尾。
 */
const props = defineProps<{
  pages: WordEntry[];
  current: number;
  /** 是否允许添加：只读或已达页数上限时禁用。 */
  canAdd?: boolean;
}>();

const emit = defineEmits<{ select: [index: number]; add: [] }>();

const query = ref("");
/** 词多时只关心没填完的页：默认显示全部，勾选后只留草稿页。 */
const incompleteOnly = ref(false);

/** 过滤后的页码与内容：过滤不改变真实页序，选中仍回传原始下标。 */
const visible = computed(() => props.pages
  .map((page, index) => ({ page, index }))
  .filter(({ page }) => {
    if (incompleteOnly.value && isEntryComplete(page)) return false;
    const keyword = query.value.trim().toLowerCase();
    if (!keyword) return true;
    return `${page.word} ${page.meaning}`.toLowerCase().includes(keyword);
  }));
</script>

<template>
  <div class="word-group-list flex h-full min-h-0 flex-col">
    <!-- 列表头只占一行：搜索 + 草稿筛选，和文件树的「标题 + 搜索」结构区分开。 -->
    <div class="flex shrink-0 items-center gap-1 border-b border-hairline px-2 py-2">
      <label class="flex min-w-0 flex-1 items-center gap-1.5 rounded-md px-1.5 hover:bg-bg-hover focus-within:bg-bg-hover">
        <Search :size="12" class="shrink-0 text-ink-3" />
        <input
          v-model="query"
          class="min-w-0 flex-1 border-0 bg-transparent py-1 text-[11px] text-ink placeholder:text-ink-3 focus-visible:!outline-none"
          :placeholder="`搜索 ${pages.length} 个单词`"
          aria-label="搜索单词"
          autocomplete="off"
          spellcheck="false"
        />
      </label>
      <button
        type="button"
        class="icon-btn !size-6"
        :class="{ active: incompleteOnly }"
        aria-label="只看未完成"
        :aria-pressed="incompleteOnly"
        :title="incompleteOnly ? '显示全部' : '只看未完成（草稿）'"
        @click="incompleteOnly = !incompleteOnly"
      >
        <ListFilter :size="13" />
      </button>
    </div>

    <ul class="soft-scrollbar min-h-0 flex-1 overflow-y-auto p-1.5">
      <li v-for="{ page, index } in visible" :key="page.id">
        <button
          type="button"
          class="flex w-full items-center gap-2 rounded-md px-2 py-1.5 text-left transition"
          :class="index === current ? 'bg-bg-active/60' : 'hover:bg-bg-hover'"
          :aria-current="index === current ? 'true' : undefined"
          :aria-label="`第 ${index + 1} 页`"
          @click="emit('select', index)"
        >
          <span class="w-5 shrink-0 text-right text-[10px] tabular-nums text-ink-3">{{ index + 1 }}</span>
          <span class="min-w-0 flex-1">
            <span class="block truncate text-[12px]" :class="page.word.trim() ? 'text-ink' : 'text-ink-3'">{{ page.word.trim() || "未填写单词" }}</span>
            <span v-if="page.meaning.trim()" class="mt-0.5 block truncate text-[10px] text-ink-3">{{ page.meaning }}</span>
          </span>
          <span
            class="size-1.5 shrink-0 rounded-full"
            :class="isEntryComplete(page) ? 'bg-accent' : 'bg-ink-3/40'"
            :title="isEntryComplete(page) ? '已完整' : '草稿：单词与释义都填好才算完整'"
          />
        </button>
      </li>
      <li v-if="!visible.length" class="px-3 py-6 text-center text-[11px] text-ink-3">
        {{ pages.length === 0 ? "还没有单词" : "没有匹配的单词" }}
      </li>
    </ul>

    <div class="shrink-0 border-t border-hairline p-1.5">
      <button type="button" class="ghost-btn w-full justify-center" :disabled="!canAdd" @click="emit('add')">
        <Plus :size="13" />添加单词
      </button>
    </div>
  </div>
</template>
