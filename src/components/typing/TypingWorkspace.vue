<script setup lang="ts">
import { ArrowLeft, Keyboard } from "@lucide/vue";
import { onBeforeUnmount, onMounted, ref, watch } from "vue";
import TypingSetup from "./TypingSetup.vue";
import TypingBoard from "./TypingBoard.vue";
import TypingStatsBar from "./TypingStatsBar.vue";
import TypingSummary from "./TypingSummary.vue";
import VirtualScrollbar from "../VirtualScrollbar.vue";
import { createTypingSession, type TypingScope } from "../../services/typingSession";
import { practiceSettings, setPracticeSettings } from "../../services/stores/typingStore";
import { resolveError } from "../../utils/errorMessage";
import type { PracticeSettings } from "../../domain/types";
import type { ConfirmRequest } from "../../composables/useConfirm";

/**
 * 打字背诵工作区：准备 → 练习 → 翻页 → 结算。
 *
 * 契约：键盘监听只在这里；工作区被遮挡（suspended）时不响应按键并暂停计时。
 * 与复习工作区一致，隐藏时保留进度，关闭即丢弃本次会话；正文用浮层滚动条，
 * 与 .soft-scrollbar 搭配隐藏 WebView 自带滚动条。
 */
const props = defineProps<{
  title: string;
  notePath: string | null;
  scope: TypingScope;
  suspended?: boolean;
  /** 退出确认：由壳层注入，练习中退出前先问一次是否放弃本次进度。 */
  confirm?: (request: ConfirmRequest) => Promise<boolean>;
}>();

const emit = defineEmits<{ close: [] }>();

/** 正文滚动容器：浮层滚动条必须与它同级渲染。 */
const bodyScroll = ref<HTMLElement | null>(null);

const session = createTypingSession({
  notePath: props.notePath,
  scope: props.scope,
  settings: () => practiceSettings.value,
});
const {
  phase,
  errorMessage,
  scope: activeScope,
  words,
  pages,
  pageIndex,
  records,
  attempt,
  combo,
  maxCombo,
  score,
  activePage,
  doneCount,
  memoryMode,
  accuracy,
  speed,
  comboMilestone,
  audioReady,
  audioProgress,
  load,
  setScope,
  rebuild,
  start,
  typeKey,
  isSpaceLiteralHere,
  finishWord,
  restart,
  setActive,
  dispose,
} = session;

const settingsSaving = ref(false);
const settingsError = ref("");
const noUndoHint = ref(false);
let noUndoTimer: number | undefined;

/** 清理「不支持回退」的提示定时器。 */
function clearNoUndoTimer(): void {
  if (noUndoTimer !== undefined) {
    window.clearTimeout(noUndoTimer);
    noUndoTimer = undefined;
  }
}

/** 第一次按退格时提示一次，避免用户以为键盘失灵。 */
function showNoUndoHint(): void {
  noUndoHint.value = true;
  clearNoUndoTimer();
  noUndoTimer = window.setTimeout(() => {
    noUndoTimer = undefined;
    noUndoHint.value = false;
  }, 2000);
}

/** 偏好只在准备页改：先持久化，再按新参数重建分页。 */
async function changeSettings(next: PracticeSettings): Promise<void> {
  if (settingsSaving.value) return;
  settingsSaving.value = true;
  settingsError.value = "";
  try {
    await setPracticeSettings(next);
    rebuild();
  } catch (error) {
    settingsError.value = resolveError(error);
  } finally {
    settingsSaving.value = false;
  }
}

/** 开始练习前先移除按钮焦点，否则空格会再次触发按钮而不是打字。 */
function startPractice(): void {
  (document.activeElement as HTMLElement | null)?.blur();
  start();
}

/** 重试加载；模板里不写 void，保持可读。 */
function reload(): void {
  void load();
}

/** 切换范围：命中缓存的词池时同步切换，发音在后台重新预取。 */
function switchScope(next: TypingScope): void {
  void setScope(next);
}

/** 结算页「调整设置」回到准备页，词池保持不变。 */
function adjust(): void {
  rebuild();
}

/** 关闭工作区：交还键盘并让壳层收起。 */
function close(): void {
  clearNoUndoTimer();
  emit("close");
}

/** 练习或翻页中退出会丢弃进度，先确认一次；准备页与结算页没有可丢的内容。 */
async function requestClose(): Promise<void> {
  if (phase.value === "drill") {
    const confirmed = await props.confirm?.({
      title: "放弃本次练习？",
      body: "当前页的作答记录不会保存，下次需要重新开始。",
      confirmLabel: "放弃练习",
      danger: true,
    }) ?? true;
    if (!confirmed) {
      return;
    }
  }
  close();
}

/** 全局按键：只在练习与翻页阶段消费，其余按键交还浏览器。 */
function handleKeydown(event: KeyboardEvent): void {
  if (props.suspended || event.isComposing) return;
  const target = event.target as HTMLElement | null;
  if (target && ["INPUT", "TEXTAREA", "SELECT", "BUTTON"].includes(target.tagName)) return;
  if (event.ctrlKey || event.metaKey || event.altKey) return;
  if (event.key === "Escape") {
    event.preventDefault();
    void requestClose();
    return;
  }
  if (phase.value !== "drill") return;
  if (event.key === "Backspace" || event.key === "Delete") {
    event.preventDefault();
    showNoUndoHint();
    return;
  }
  if (event.key === " " || event.key === "Enter") {
    event.preventDefault();
    if (event.key === " " && isSpaceLiteralHere()) typeKey(" ");
    else finishWord();
    return;
  }
  if (event.key.length === 1) {
    event.preventDefault();
    typeKey(event.key);
  }
}

/** 被设置 / 命令面板等遮挡时暂停；恢复后继续走表。 */
watch(() => props.suspended, (suspended) => setActive(!suspended), { immediate: true });

onMounted(() => {
  void load();
  window.addEventListener("keydown", handleKeydown);
});

onBeforeUnmount(() => {
  window.removeEventListener("keydown", handleKeydown);
  clearNoUndoTimer();
  dispose();
});
</script>

<template>
  <main class="relative flex min-h-0 min-w-0 flex-1" aria-label="打字背诵工作区">
    <div class="flex min-h-0 min-w-0 flex-1 flex-col bg-bg-paper">
      <header class="flex h-12 shrink-0 items-center gap-3 border-b border-hairline px-3">
        <button type="button" class="ghost-btn" @click="requestClose">
          <ArrowLeft :size="14" />返回
        </button>
        <p class="min-w-0 flex-1 truncate text-center text-[12px] font-medium">{{ title }}</p>
        <span v-if="phase === 'drill'" class="shrink-0 text-[11px] tabular-nums text-ink-3">
          第 {{ pageIndex + 1 }} / {{ pages.length }} 页
        </span>
        <span v-else-if="phase === 'setup'" class="shrink-0 text-[11px] text-ink-3">
          每词 {{ practiceSettings.repeatsPerWord }} 遍 · 每页 {{ practiceSettings.wordsPerPage }} 词
        </span>
        <span v-else class="w-16 shrink-0" />
      </header>

      <div ref="bodyScroll" class="soft-scrollbar min-h-0 flex-1 overflow-y-auto">
        <section v-if="phase === 'loading'" class="flex min-h-full items-center justify-center px-6">
          <p class="text-[13px] text-ink-3">正在加载单词…</p>
        </section>

        <section v-else-if="phase === 'error'" class="flex min-h-full items-center justify-center px-6">
          <div class="text-center">
            <p class="text-[13px] text-danger">{{ errorMessage }}</p>
            <div class="mt-4 flex justify-center gap-2">
              <button type="button" class="ghost-btn" @click="close">返回</button>
              <button type="button" class="primary-btn" @click="reload">重试</button>
            </div>
          </div>
        </section>

        <section v-else-if="phase === 'empty'" class="flex min-h-full items-center justify-center px-6">
          <div class="max-w-[420px] text-center">
            <Keyboard :size="28" class="mx-auto text-ink-3" />
            <p class="mt-4 text-[14px] font-medium">这个范围里还没有单词卡</p>
            <p class="mt-1 text-[12px] leading-5 text-ink-3">先在笔记里用 AI 拆卡或手动添加单词卡，再回来练习。</p>
            <div class="mt-6 flex justify-center gap-2">
              <button v-if="activeScope === 'note'" type="button" class="ghost-btn border border-hairline" @click="switchScope('all')">
                切换到全部单词卡
              </button>
              <button type="button" class="ghost-btn" @click="close">返回</button>
            </div>
          </div>
        </section>

        <section v-else-if="phase === 'setup'" class="px-4 py-6">
          <TypingSetup
            :scope="activeScope"
            :word-count="words.length"
            :settings="practiceSettings"
            :saving="settingsSaving"
            :error="settingsError"
            :can-switch-scope="Boolean(notePath)"
            :audio-ready="audioReady"
            :audio-progress="audioProgress"
            @start="startPractice"
            @scope="switchScope"
            @settings="changeSettings"
          />
        </section>

        <section v-else-if="phase === 'finished'" class="flex min-h-full flex-col">
          <TypingSummary
            :word-count="words.length"
            :page-count="pages.length"
            :score="score"
            :max-combo="maxCombo"
            :accuracy="accuracy"
            :speed="speed"
            :perfect-words="session.totals.perfectWords"
            :settings="practiceSettings"
            @restart="restart"
            @adjust="adjust"
            @close="close"
          />
        </section>

        <section v-else class="px-4 py-4">
          <div class="mb-3 flex flex-wrap items-center justify-between gap-2">
            <TypingStatsBar
              :combo="combo"
              :max-combo="maxCombo"
              :score="score"
              :accuracy="accuracy"
              :speed="speed"
              :combo-milestone="comboMilestone"
            />
            <span class="text-[10px] text-ink-3">每词 {{ practiceSettings.repeatsPerWord }} 遍 · 每页 {{ practiceSettings.wordsPerPage }} 词</span>
          </div>
          <TypingBoard
            v-if="activePage"
            :page="activePage"
            :phase="phase"
            :done-count="doneCount"
            :attempt="attempt"
            :records="records"
            :memory-mode="memoryMode"
          />
          <p v-if="noUndoHint" class="mt-3 text-center text-[11px] text-ink-3">打字背诵不支持回退，打错继续往前打就行</p>
          <p v-if="errorMessage" class="mt-3 text-center text-[11px] text-danger">{{ errorMessage }}</p>
        </section>
      </div>
    </div>
    <VirtualScrollbar :target="bodyScroll" />
  </main>
</template>
