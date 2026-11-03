<script setup lang="ts">
import { computed, onBeforeUnmount, ref } from "vue";
import { Search } from "@lucide/vue";
import { isEntryComplete, normalizePhonetic, WORD_ENTRY_LIMITS, type WordEntry } from "../../domain/wordGroup";
import type { DictionaryEntry } from "../../domain/types";
import { usePhoneticLookup } from "../../composables/usePhoneticLookup";

/**
 * 单词组单页表单：单词 / 音标 / 释义 / 例句 + 音标自动查询。
 *
 * 契约：组件不持有页面状态，每次编辑上抛完整页面对象，由编辑器统一序列化保存；
 * 音标只在为空时被自动填入，手填内容永远优先，"查询音标"按钮才允许覆盖。
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

/** 完整页会物化卡片；提示文案让用户知道"填到哪里才有卡"。 */
const complete = computed(() => isEntryComplete(props.entry));

/** 整页上抛：保持对象形状固定，编辑器据此立即序列化。 */
function patch(field: keyof WordEntry, event: Event): void {
  const value = (event.target as HTMLInputElement | HTMLTextAreaElement).value;
  emit("update:entry", { ...props.entry, [field]: value });
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
  <div class="word-page-form mt-4 space-y-3">
    <div class="grid gap-3 md:grid-cols-2">
      <label class="block">
        <span class="mb-1 block text-[11px] text-ink-3">单词</span>
        <input
          ref="wordInput"
          :value="entry.word"
          :maxlength="WORD_ENTRY_LIMITS.word"
          :readonly="readOnly"
          class="field-input w-full"
          placeholder="例如 ephemeral"
          aria-label="单词"
          autocomplete="off"
          spellcheck="false"
          @input="inputWord"
          @compositionstart="compositionStart"
          @compositionend="compositionEnd"
          @keydown.enter.prevent="queryPhonetic"
          @blur="onWordBlur"
        />
      </label>
      <label class="block">
        <span class="mb-1 flex items-center justify-between gap-2 text-[11px] text-ink-3">
          <span>音标</span>
          <span aria-live="polite">{{ lookupHint }}</span>
        </span>
        <span class="flex gap-1.5">
          <input
            :value="entry.phonetic"
            :maxlength="WORD_ENTRY_LIMITS.phonetic"
            :readonly="readOnly"
            class="field-input min-w-0 flex-1"
            placeholder="/ɪˈfemərəl/"
            aria-label="音标"
            autocomplete="off"
            @input="patch('phonetic', $event)"
          />
          <button
            type="button"
            class="ghost-btn shrink-0 justify-center border border-hairline"
            :disabled="readOnly || !entry.word.trim() || lookupStatus === 'loading'"
            title="查询 ECDICT 并覆盖当前音标"
            @click="queryPhonetic"
          >
            <Search :size="13" />查询音标
          </button>
        </span>
      </label>
    </div>

    <label class="block">
      <span class="mb-1 block text-[11px] text-ink-3">释义</span>
      <textarea
        :value="entry.meaning"
        :maxlength="WORD_ENTRY_LIMITS.meaning"
        :readonly="readOnly"
        rows="3"
        class="field-input w-full resize-y"
        placeholder="例如 短暂的；转瞬即逝的"
        aria-label="释义"
        @input="patch('meaning', $event)"
      />
    </label>

    <label class="block">
      <span class="mb-1 block text-[11px] text-ink-3">例句</span>
      <textarea
        :value="entry.example"
        :maxlength="WORD_ENTRY_LIMITS.example"
        :readonly="readOnly"
        rows="3"
        class="field-input w-full resize-y"
        placeholder="例如 Fame in the digital age can be remarkably ephemeral."
        aria-label="例句"
        @input="patch('example', $event)"
      />
    </label>

    <p class="text-[11px]" :class="complete ? 'text-accent-strong' : 'text-ink-3'" role="status">
      {{ complete ? "已生成卡片：保存后可在右侧复习。" : "填写单词与释义后自动生成卡片。" }}
    </p>
  </div>
</template>
