<script setup lang="ts">
import { ChevronLeft, ChevronRight } from "@lucide/vue";

/**
 * 单词组分页控件：上一页 / 页码下拉 / 下一页。
 *
 * 契约：只上抛页码变化意图；越界与只读判定在编辑器里，组件自身不持有页码状态。
 */
const props = defineProps<{
  /** 当前页下标（0 起始）。 */
  page: number;
  /** 总页数；0 表示空组。 */
  count: number;
  readOnly?: boolean;
}>();

const emit = defineEmits<{ change: [page: number] }>();

/** 下拉选择换算成 0 起始下标；越界或非法值不提交。 */
function selectPage(event: Event): void {
  const next = Number((event.target as HTMLSelectElement).value);
  if (Number.isInteger(next) && next >= 0 && next < props.count) {
    emit("change", next);
  }
}
</script>

<template>
  <div class="word-group-pager flex items-center gap-1 text-[11px] text-ink-3">
    <button type="button" class="icon-btn !size-7" aria-label="上一页" title="上一页（Alt+←）" :disabled="readOnly || page <= 0" @click="emit('change', page - 1)">
      <ChevronLeft :size="15" />
    </button>
    <select
      class="field-input !h-7 w-20 px-1 text-center text-[11px]"
      aria-label="跳转页码"
      :value="String(page)"
      :disabled="readOnly || count === 0"
      @change="selectPage"
    >
      <option v-for="index in count" :key="index" :value="String(index - 1)">第 {{ index }} 页</option>
    </select>
    <button type="button" class="icon-btn !size-7" aria-label="下一页" title="下一页（Alt+→）" :disabled="readOnly || page >= count - 1" @click="emit('change', page + 1)">
      <ChevronRight :size="15" />
    </button>
    <span class="tabular-nums">{{ count === 0 ? "0 / 0" : `${page + 1} / ${count}` }}</span>
  </div>
</template>
