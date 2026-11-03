<script setup lang="ts">
import { computed, nextTick, onBeforeUnmount, onMounted, ref, watch } from "vue";
import { Brain, Keyboard, Plus, Trash2 } from "@lucide/vue";
import NoteTabs from "../NoteTabs.vue";
import NoteToolbar from "../NoteToolbar.vue";
import WordGroupPager from "./WordGroupPager.vue";
import WordPageForm from "./WordPageForm.vue";
import { documentTitleOf } from "../../domain/documentKind";
import {
  createEntry,
  parseWordGroup,
  serializeWordGroup,
  WORD_GROUP_FORMAT_VERSION,
  WORD_GROUP_MAX_PAGES,
  type WordEntry,
} from "../../domain/wordGroup";
import type { DictionaryEntry } from "../../domain/types";

/**
 * 单词组分页编辑器：一页一个单词，编辑即序列化整份 JSON 交给保存服务。
 *
 * 契约：
 * - 标签行复用 NoteTabs + NoteToolbar，但永远不提供阅读模式（组不是 Markdown 正文）。
 * - 解析失败只读展示，绝不写盘；空组仍允许添加单词。
 * - 本组件是唯一序列化入口，保存服务只负责防抖写盘，后端负责卡片物化。
 */
const props = defineProps<{
  notePath: string;
  /** 磁盘内容（JSON 文本）；外部重载时覆盖本地状态，自己保存的回显会被忽略。 */
  content: string;
  panelOpen: boolean;
  readOnly?: boolean;
  tabs?: { path: string; title: string }[];
  tabsBusy?: boolean;
  /** 注入词典查询便于测试；缺省由页面表单走后端 ECDICT。 */
  lookupWord?: (word: string) => Promise<DictionaryEntry | null>;
}>();

const emit = defineEmits<{
  "select-tab": [path: string];
  "close-tab": [path: string];
  "toggle-panel": [];
  "save-content": [notePath: string, content: string];
  "start-review": [];
  "start-typing": [];
}>();

const pages = ref<WordEntry[]>([]);
const parseError = ref("");
const current = ref(0);
const form = ref<InstanceType<typeof WordPageForm> | null>(null);
/** 最近一次由本组件序列化并上抛的内容：用于区分"自己保存的回显"与"外部重载"。 */
let lastSerialized = "";

const title = computed(() => documentTitleOf(props.notePath));
const pageCount = computed(() => pages.value.length);
const currentEntry = computed<WordEntry | null>(() => pages.value[current.value] ?? null);
const readOnly = computed(() => Boolean(props.readOnly) || parseError.value.length > 0);
const pageLabel = computed(() => (pageCount.value === 0 ? "还没有单词" : `第 ${current.value + 1} / ${pageCount.value} 页`));

/** 载入磁盘内容：只接受基本形状合法的文档，失败时清空并进入只读。 */
function load(content: string): void {
  const result = parseWordGroup(content);
  if (!result.ok) {
    pages.value = [];
    parseError.value = result.error;
    current.value = 0;
    return;
  }
  parseError.value = "";
  pages.value = result.document.words;
  if (current.value >= pages.value.length) {
    current.value = Math.max(0, pages.value.length - 1);
  }
}
load(props.content);

/** 外部重载（重扫、改名为当前路径等）覆盖本地状态；自己保存的回显不打断输入。 */
watch(() => props.content, (content) => {
  if (content === lastSerialized) return;
  load(content);
});

/** 切换到另一个单词组：重置序列化标记并从新内容载入，空组的相同 JSON 也不会被误判为回显。 */
watch(() => props.notePath, () => {
  current.value = 0;
  lastSerialized = "";
  load(props.content);
});

/** 序列化整份文档并上抛：保存服务防抖写盘，后端写盘后物化卡片。 */
function touch(): void {
  if (readOnly.value) return;
  lastSerialized = serializeWordGroup({ formatVersion: WORD_GROUP_FORMAT_VERSION, words: pages.value });
  emit("save-content", props.notePath, lastSerialized);
}

/** 更新当前页字段：本地先改再序列化，翻页不会丢正在输入的内容。 */
function updateEntry(entry: WordEntry): void {
  if (readOnly.value) return;
  const index = current.value;
  if (!pages.value[index]) return;
  pages.value[index] = entry;
  touch();
}

/** 添加一页草稿并跳到新页，聚焦单词输入。 */
async function addPage(): Promise<void> {
  if (readOnly.value || pages.value.length >= WORD_GROUP_MAX_PAGES) return;
  pages.value = [...pages.value, createEntry()];
  current.value = pages.value.length - 1;
  touch();
  await focusWord();
}

/** 删除当前页：按钮 title 已提示会删除该页复习记录，属于显式操作故不再二次确认。 */
function deletePage(): void {
  if (readOnly.value || pages.value.length === 0) return;
  const index = current.value;
  pages.value = pages.value.filter((_, at) => at !== index);
  current.value = Math.max(0, Math.min(index, pages.value.length - 1));
  touch();
  void focusWord();
}

/** 翻页：越界夹到首末页；内容已在编辑时保存，切换只移动焦点。 */
function goTo(index: number): void {
  const next = Math.max(0, Math.min(index, pages.value.length - 1));
  if (next === current.value) return;
  current.value = next;
  void focusWord();
}

/** 翻页后把焦点交给单词输入，方便连续录入。 */
async function focusWord(): Promise<void> {
  await nextTick();
  form.value?.focusWord();
}

/**
 * 翻页快捷键：Alt+←/→ 与 Home/End。
 * 焦点在输入框/文本域时一律让位给文本编辑（Home/End 与 Alt+方向键有原生语义）。
 */
function handleKeydown(event: KeyboardEvent): void {
  if (event.isComposing || readOnly.value || pageCount.value === 0) return;
  const target = event.target as HTMLElement | null;
  // 输入框、文本域、下拉框与可编辑区都有自己的 Home/End 语义，一律不抢。
  if (target instanceof HTMLInputElement || target instanceof HTMLTextAreaElement
    || target instanceof HTMLSelectElement || target?.isContentEditable) return;
  if (event.altKey && !event.ctrlKey && !event.metaKey && (event.key === "ArrowLeft" || event.key === "ArrowRight")) {
    event.preventDefault();
    goTo(event.key === "ArrowLeft" ? current.value - 1 : current.value + 1);
    return;
  }
  if (event.key === "Home") {
    event.preventDefault();
    goTo(0);
    return;
  }
  if (event.key === "End") {
    event.preventDefault();
    goTo(pageCount.value - 1);
  }
}

onMounted(() => {
  window.addEventListener("keydown", handleKeydown);
  // 打开或新建单词组后直接开始录入：焦点放到单词输入，不用先点一下。
  void focusWord();
});
onBeforeUnmount(() => window.removeEventListener("keydown", handleKeydown));
</script>

<template>
  <div class="word-group-editor relative flex h-full min-h-0 flex-col overflow-hidden" :class="{ 'has-tabs': tabs?.length }">
    <NoteTabs v-if="tabs?.length" :tabs="tabs" :active-path="notePath" :busy="tabsBusy" @select="emit('select-tab', $event)" @close="emit('close-tab', $event)">
      <template #actions>
        <NoteToolbar inline :panel-open="panelOpen" :reading-enabled="false" @toggle-panel="emit('toggle-panel')" />
      </template>
    </NoteTabs>
    <NoteToolbar v-else :panel-open="panelOpen" :reading-enabled="false" @toggle-panel="emit('toggle-panel')" />

    <div class="soft-scrollbar min-h-0 flex-1 overflow-y-auto px-6 py-4">
      <div class="mx-auto flex h-full w-full max-w-3xl flex-col">
        <header class="flex flex-wrap items-center justify-between gap-2 border-b border-hairline pb-3">
          <div class="min-w-0">
            <p class="truncate text-[15px] font-semibold">{{ title }}</p>
            <p class="text-[11px] text-ink-3">{{ pageLabel }} · 单词组</p>
          </div>
          <div class="flex flex-wrap items-center gap-1.5">
            <WordGroupPager :page="current" :count="pageCount" :read-only="readOnly" @change="goTo" />
            <button type="button" class="ghost-btn justify-center border border-hairline" :disabled="readOnly" @click="void addPage()">
              <Plus :size="13" />添加单词
            </button>
            <button
              type="button"
              class="ghost-btn justify-center border border-hairline"
              title="删除本页；该页的复习记录会一并删除"
              :disabled="readOnly || pageCount === 0"
              @click="deletePage"
            >
              <Trash2 :size="13" />删除本页
            </button>
            <button
              type="button"
              class="ghost-btn justify-center border border-hairline"
              title="看中文打英文，逐字判定并记忆巩固"
              :disabled="pageCount === 0"
              @click="emit('start-typing')"
            >
              <Keyboard :size="13" />打字背诵
            </button>
            <button type="button" class="primary-btn" :disabled="pageCount === 0" @click="emit('start-review')">
              <Brain :size="14" />复习本组
            </button>
          </div>
        </header>

        <p v-if="parseError" role="alert" class="mt-3 rounded-md border border-danger/40 bg-danger/5 px-3 py-2 text-[12px] text-danger">
          {{ parseError }}
        </p>

        <WordPageForm
          v-if="currentEntry"
          :key="currentEntry.id"
          ref="form"
          :entry="currentEntry"
          :read-only="readOnly"
          :lookup-word="lookupWord"
          @update:entry="updateEntry"
        />

        <div v-else-if="!parseError" class="flex flex-1 flex-col items-center justify-center py-16 text-center">
          <p class="text-[13px] text-ink-2">还没有单词</p>
          <p class="mt-1 text-[11px] text-ink-3">添加一页，填好单词与释义后会自动生成可复习的卡片。</p>
          <button type="button" class="primary-btn mt-4" @click="void addPage()">
            <Plus :size="14" />添加单词
          </button>
        </div>
      </div>
    </div>
  </div>
</template>
