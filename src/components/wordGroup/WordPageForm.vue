<script setup lang="ts">
import { computed, onBeforeUnmount, ref } from "vue";
import { Search } from "@lucide/vue";
import { normalizePhonetic, WORD_ENTRY_LIMITS, type WordEntry } from "../../domain/wordGroup";
import type { DictionaryEntry } from "../../domain/types";
import { usePhoneticLookup } from "../../composables/usePhoneticLookup";

/**
 * 单词组单页表单：单词 / 音标 / 释义 / 例句 + 音标自动查询。
 *
 * 契约：组件不持有页面状态，每次编辑上抛完整页面对象，由编辑器统一序列化保存；
 * 音标只在为空时被自动填入，手填内容永远优先，"查询音标"按钮才允许覆盖。
 * 释义与例句铺满表单剩余高度：一页只有一个词，固定行数会在下方留出大片空白。
 * 页面完成度（草稿/已生成）由编辑器头部统一展示，表单内不重复提示。
 */
const props = defineProps<{
  entry: WordEntry;
  readOnly?: boolean;
  /** 注入词典查询便于测试；缺省由组合函数走后端 ECDICT。 */
  lookupWord?: (word: string) => Promise<DictionaryEntry | null>;
}>();

const emit = defineEmits<{ "update:entry": [entry: WordEntry] }>();

const wordInput = ref<HTMLInputElement | null>(null);
/** 中文输入法组合期间不触发查询；确认候选后由 compositionend 补一次。 */
let composing = false;
/** 只有显式点击「查询音标」才允许覆盖手填内容。 */
let forceFill = false;

const lookup = usePhoneticLookup({
  lookup: props.lookupWord,
  onResult: (result) => {
    const override = forceFill;
    forceFill = false;
    const phonetic = normalizePhonetic(result?.phonetic ?? "");
    if (!phonetic) return;
    // 自动查询只填空音标；显式按钮才会覆盖用户手填的值。
    if (!override && props.entry.phonetic.trim()) return;
    emit("update:entry", { ...props.entry, phonetic });
  },
});
const { status: lookupStatus } = lookup;

/** 查询状态文案：失败与未收录都明确告知可手填，不弹错误。 */
const lookupHint = computed(() => {
  if (lookupStatus.value === "loading") return "查询中…";
  if (lookupStatus.value === "ready") return "已填入音标";
  if (lookupStatus.value === "missing") return "词典未收录，可手填";
  if (lookupStatus.value === "error") return "查询失败，可手填";
  return "";
});

/** 整页上抛：保持对象形状固定，编辑器据此立即序列化。 */
function patch(field: keyof WordEntry, event: Event): void {
  const value = (event.target as HTMLInputElement | HTMLTextAreaElement).value;
  emit("update:entry", { ...props.entry, [field]: value });
}

/** 释义与例句：直接上抛内容，高度由布局撑满，不再按内容逐次调整。 */
function inputText(field: "meaning" | "example", event: Event): void {
  patch(field, event);
}

/** 单词输入：先发布内容，再按组合输入状态安排防抖查询。 */
function inputWord(event: Event): void {
  const value = (event.target as HTMLInputElement).value;
  emit("update:entry", { ...props.entry, word: value });
  lookup.input(value, composing || (event as InputEvent).isComposing);
}

/** 组合输入开始：只暂停查询，不阻塞文本输入。 */
function compositionStart(): void {
  composing = true;
}

/** 组合输入结束：用最终文本补一次查询安排。 */
function compositionEnd(event: CompositionEvent): void {
  composing = false;
  lookup.input((event.target as HTMLInputElement).value, false);
}

/** 回车或按钮：跳过防抖立即查询，并允许覆盖手填音标。 */
function queryPhonetic(): void {
  if (props.readOnly || !props.entry.word.trim()) return;
  forceFill = true;
  void lookup.lookupNow(props.entry.word);
}

/** 失焦补查：单词已填、音标仍空且还没查到结果时补一次。 */
function onWordBlur(): void {
  if (props.readOnly) return;
  const word = props.entry.word.trim();
  if (!word || props.entry.phonetic.trim()) return;
  if (lookupStatus.value === "loading" || lookupStatus.value === "ready") return;
  void lookup.lookupNow(word);
}

/** 翻页后由编辑器聚焦单词输入，直接开始录入下一页。 */
function focusWord(): void {
  wordInput.value?.focus();
  wordInput.value?.select();
}

defineExpose({ focusWord });
onBeforeUnmount(() => lookup.dispose());
</script>

<template>
  <div class="word-page-form flex min-h-0 flex-1 flex-col gap-4">
    <!-- 第一行就是单词：大字无框，音标紧随其后，本页的删除与保存状态由编辑器放进 actions 插槽。 -->
    <div class="flex shrink-0 flex-wrap items-center gap-x-3 gap-y-1">
      <input
        ref="wordInput"
        :value="entry.word"
        :maxlength="WORD_ENTRY_LIMITS.word"
        :readonly="readOnly"
        class="min-w-0 flex-1 border-0 border-b border-transparent bg-transparent pb-1 !text-[26px] leading-tight font-semibold text-ink placeholder:font-normal placeholder:text-ink-3/60 focus:border-accent focus-visible:!outline-none"
        placeholder="输入单词"
        aria-label="单词"
        autocomplete="off"
        spellcheck="false"
        @input="inputWord"
        @compositionstart="compositionStart"
        @compositionend="compositionEnd"
        @keydown.enter.prevent="queryPhonetic"
        @blur="onWordBlur"
      />
      <input
        :value="entry.phonetic"
        :maxlength="WORD_ENTRY_LIMITS.phonetic"
        :readonly="readOnly"
        class="w-44 shrink-0 rounded border border-transparent bg-transparent px-1 py-0.5 !text-[13px] text-ink-2 placeholder:!text-[12px] placeholder:text-ink-3/60 hover:border-hairline focus:border-hairline focus-visible:!outline-none"
        placeholder="音标"
        aria-label="音标"
        autocomplete="off"
        @input="patch('phonetic', $event)"
      />
      <button
        type="button"
        class="icon-btn shrink-0"
        aria-label="查询音标"
        title="查询 ECDICT 并覆盖当前音标（单词框内回车同效）"
        :disabled="readOnly || !entry.word.trim() || lookupStatus === 'loading'"
        @click="queryPhonetic"
      >
        <Search :size="15" />
      </button>
      <span class="shrink-0 text-[11px] text-ink-3" aria-live="polite">{{ lookupHint }}</span>
      <div class="ml-auto flex shrink-0 items-center gap-1.5">
        <slot name="actions" />
      </div>
    </div>

    <!-- 宽窗口下释义与例句并排，避免右侧留出大片空白；窄窗口自动退回上下排列。 -->
    <div class="grid min-h-0 flex-1 gap-4 md:grid-cols-2">
      <label class="flex min-h-0 flex-col">
        <span class="mb-1.5 block text-[11px] text-ink-3">释义</span>
        <textarea
          :value="entry.meaning"
          :maxlength="WORD_ENTRY_LIMITS.meaning"
          :readonly="readOnly"
          class="field-input min-h-[64px] w-full flex-1 resize-none py-2 leading-6"
          placeholder="例如 短暂的；转瞬即逝的"
          aria-label="释义"
          @input="inputText('meaning', $event)"
        />
      </label>

      <label class="flex min-h-0 flex-col">
        <span class="mb-1.5 block text-[11px] text-ink-3">例句<span class="ml-1.5 text-[10px] text-ink-3/70">可选</span></span>
        <textarea
          :value="entry.example"
          :maxlength="WORD_ENTRY_LIMITS.example"
          :readonly="readOnly"
          class="field-input min-h-[64px] w-full flex-1 resize-none py-2 leading-6"
          placeholder="例如 Fame in the digital age can be remarkably ephemeral."
          aria-label="例句"
          @input="inputText('example', $event)"
        />
      </label>
    </div>
  </div>
</template>
