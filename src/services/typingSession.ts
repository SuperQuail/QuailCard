import { computed, reactive, ref } from "vue";
import { getReviewQueue } from "./stores/reviewStore";
import { resolveError } from "../utils/errorMessage";
import {
  accuracyOf,
  buildTypingPages,
  buildTypingWords,
  finishAttempt,
  isSpaceLiteral,
  nextCombo,
  scoreGain,
  startAttempt,
  summarizeAttempt,
  typeCharacter,
  type AttemptSummary,
  type TypingAttempt,
  type TypingItem,
  type TypingPage,
  type TypingRecord,
  type TypingWord,
} from "../domain/typingDrill";
import type { PracticeSettings } from "../domain/types";
import { cacheAudioInBackground, hasCachedAudio, playCachedAudio, preloadAudio } from "./typingAudio";

/**
 * 打字背诵会话：把领域规则接到真实复习队列上，并持有页面、当前格与统计。
 *
 * 契约：本模块只调用复习域的只读队列接口，绝不提交评分；
 * 练习成绩只影响本次会话的连击与统计，不触碰卡片的调度状态。
 */

/** 练习范围：当前笔记或整个 Vault。 */
export type TypingScope = "note" | "all";

/** 工作区阶段；drill 是练习，finished 是结算。 */
export type TypingPhase = "loading" | "setup" | "drill" | "finished" | "empty" | "error";

/** 会话累计统计：字母对错、完成词数与完美词数。 */
export interface TypingTotals {
  correct: number;
  wrong: number;
  missed: number;
  wordsDone: number;
  perfectWords: number;
}

/** 进入练习前预取发音的单词数；预取完成前开始按钮保持锁定。 */
export const PRELOAD_WORD_COUNT = 10;

/** 发音预取进度：完成数 / 总数。 */
export interface TypingAudioProgress {
  done: number;
  total: number;
}

export interface TypingSessionOptions {
  notePath: string | null;
  scope: TypingScope;
  /** 读取当前偏好；只在开场与重建分页时调用，练习中不响应变化。 */
  settings: () => PracticeSettings;
}

/** 速度统计的刷新间隔；只在练习中走表。 */
const CLOCK_MS = 500;

/** 创建打字背诵会话。 */
export function createTypingSession(options: TypingSessionOptions) {
  const phase = ref<TypingPhase>("loading");
  const errorMessage = ref("");
  const scope = ref<TypingScope>(options.scope);
  /** 词池快照：开场固定，练习期间不随卡片增删变化。 */
  const words = ref<TypingWord[]>([]);
  const pages = ref<TypingPage[]>([]);
  const pageIndex = ref(0);
  const itemIndex = ref(0);
  /** 已完成作答的记录，按练习项 id 存放，用于「前面的记录不清」。 */
  const records = reactive<Record<string, TypingRecord>>({});
  const attempt = ref<TypingAttempt | null>(null);
  const combo = ref(0);
  const maxCombo = ref(0);
  const score = ref(0);
  const totals = reactive<TypingTotals>({ correct: 0, wrong: 0, missed: 0, wordsDone: 0, perfectWords: 0 });
  /** 首次击键以后才开始计时，避免准备时间拉低速读。 */
  const startedAt = ref<number | null>(null);
  const pausedAt = ref<number | null>(null);
  const pausedTotal = ref(0);
  /** 走表时钟：让速度在没有新击键时也会前进。 */
  const clock = ref(Date.now());
  /** 发音预取是否完成；完成前不允许进入正式练习。 */
  const audioReady = ref(false);
  const audioProgress = reactive<TypingAudioProgress>({ done: 0, total: 0 });
  /** 按范围缓存词池：来回切「本笔记 / 全部」不再重新拉全量卡片，消除切换卡顿。 */
  const poolCache = new Map<TypingScope, TypingWord[]>();
  /** 同一范围的并发请求复用同一个 Promise，避免重复拉取。 */
  const poolRequests = new Map<TypingScope, Promise<TypingWord[]>>();

  let clockTimer: number | undefined;
  let audioToken = 0;
  let disposed = false;

  const activePage = computed<TypingPage | null>(() => pages.value[pageIndex.value] ?? null);
  const activeItem = computed<TypingItem | null>(() => activePage.value?.items[itemIndex.value] ?? null);
  /** 本页已完成格数：当前格之前的格子都算完成。 */
  const doneCount = computed(() => itemIndex.value);
  /** 记忆遍：释义、原词与本页已完成记录都要模糊。 */
  const memoryMode = computed(() => phase.value === "drill" && activeItem.value?.memory === true);
  const elapsedMs = computed(() => {
    if (startedAt.value === null) return 0;
    const end = pausedAt.value ?? clock.value;
    return Math.max(0, end - startedAt.value - pausedTotal.value);
  });
  const accuracy = computed(() => accuracyOf(totals));
  const speed = computed(() => {
    const minutes = elapsedMs.value / 60_000;
    return minutes > 0 ? Math.round(totals.correct / minutes) : 0;
  });
  /** 只在达到里程碑时触发连击特效，避免每一击都闪。 */
  const comboMilestone = computed(() => [5, 10, 20, 50].includes(combo.value));

  /** 停表：结算、翻页与暂停都走这里。 */
  function stopClock(): void {
    if (clockTimer !== undefined) {
      window.clearInterval(clockTimer);
      clockTimer = undefined;
    }
  }

  /** 走表：已走表时保持单一定时器，避免重复注册。 */
  function startClock(): void {
    if (clockTimer !== undefined) return;
    clock.value = Date.now();
    clockTimer = window.setInterval(() => {
      clock.value = Date.now();
    }, CLOCK_MS);
  }

  /** 重置成绩与位置；词池与分页保持不变。 */
  function resetProgress(): void {
    stopClock();
    pageIndex.value = 0;
    itemIndex.value = 0;
    attempt.value = null;
    combo.value = 0;
    maxCombo.value = 0;
    score.value = 0;
    totals.correct = 0;
    totals.wrong = 0;
    totals.missed = 0;
    totals.wordsDone = 0;
    totals.perfectWords = 0;
    startedAt.value = null;
    pausedAt.value = null;
    pausedTotal.value = 0;
    for (const key of Object.keys(records)) delete records[key];
  }

  /** 按当前偏好重建分页并回到准备态；偏好只在准备页生效。 */
  function rebuild(): void {
    const settings = options.settings();
    pages.value = buildTypingPages(words.value, settings.repeatsPerWord, settings.wordsPerPage);
    resetProgress();
    phase.value = words.value.length === 0 ? "empty" : "setup";
  }

  /** 取指定范围的词池：命中缓存直接返回，进行中的请求复用同一个 Promise。 */
  function fetchWords(target: TypingScope): Promise<TypingWord[]> {
    const cached = poolCache.get(target);
    if (cached) {
      return Promise.resolve(cached);
    }
    const inflight = poolRequests.get(target);
    if (inflight) {
      return inflight;
    }
    const path = target === "note" ? options.notePath : null;
    const request = getReviewQueue(path, true)
      .then((cards) => {
        const loaded = buildTypingWords(cards);
        poolCache.set(target, loaded);
        poolRequests.delete(target);
        return loaded;
      })
      .catch((error: unknown) => {
        poolRequests.delete(target);
        throw error;
      });
    poolRequests.set(target, request);
    return request;
  }

  /** 后台预取另一个范围的词池：切换范围时要等 IPC 拉全量卡片，是「卡一下」的主因。 */
  function prefetchOtherScope(): void {
    const other: TypingScope = scope.value === "note" ? "all" : "note";
    if (other === "note" && !options.notePath) {
      return;
    }
    void fetchWords(other).catch(() => {
      // 预取失败不影响当前范围；切过去时再走一次正常加载与错误提示。
    });
  }

  /** 预取前 N 个单词的发音；完成前 audioReady 为 false，开始按钮保持锁定。 */
  async function prepareAudio(): Promise<void> {
    const token = (audioToken += 1);
    audioReady.value = false;
    audioProgress.done = 0;
    audioProgress.total = Math.min(PRELOAD_WORD_COUNT, words.value.length);
    const first = words.value.slice(0, PRELOAD_WORD_COUNT).map((word) => word.word);
    try {
      await preloadAudio(first, (done, total) => {
        if (token !== audioToken) return;
        audioProgress.done = done;
        audioProgress.total = total;
      });
    } finally {
      // 即使全部合成失败也解锁：发音可以缺席，练习不能被网络卡住。
      if (token === audioToken && !disposed) {
        audioReady.value = true;
      }
    }
  }

  /** 应用新词池：重建分页并准备发音；返回进度 Promise 以便首次加载等待就绪。 */
  function applyWords(): Promise<void> {
    rebuild();
    if (words.value.length === 0) {
      audioReady.value = false;
      audioProgress.done = 0;
      audioProgress.total = 0;
      return Promise.resolve();
    }
    return prepareAudio();
  }

  /** 读取练习词池：包含未到期卡片，只保留单词卡（过滤在领域层）。 */
  async function load(): Promise<void> {
    phase.value = "loading";
    errorMessage.value = "";
    try {
      const loaded = await fetchWords(scope.value);
      if (disposed) return;
      words.value = loaded;
      prefetchOtherScope();
      // 首次加载等待发音预取完成：准备页会显示进度，开始按钮在完成前不可用。
      await applyWords();
    } catch (error) {
      if (disposed) return;
      errorMessage.value = resolveError(error);
      phase.value = "error";
    }
  }

  /** 切换范围：命中缓存时同步切换，未命中时短暂加载；发音在后台重新预取。 */
  async function setScope(next: TypingScope): Promise<void> {
    if (scope.value === next) return;
    scope.value = next;
    const cached = poolCache.get(next);
    if (cached) {
      words.value = cached;
      void applyWords();
      return;
    }
    phase.value = "loading";
    errorMessage.value = "";
    try {
      const loaded = await fetchWords(next);
      if (disposed || scope.value !== next) return;
      words.value = loaded;
      void applyWords();
    } catch (error) {
      if (disposed) return;
      errorMessage.value = resolveError(error);
      phase.value = "error";
    }
  }

  /** 开始当前格作答。 */
  function beginAttempt(): void {
    const item = activeItem.value;
    attempt.value = item ? startAttempt(item) : null;
  }

  /** 开始一轮练习：发音预取完成前不允许进入，成绩与位置一起重置。 */
  function start(): void {
    if (pages.value.length === 0 || !audioReady.value) {
      return;
    }
    resetProgress();
    phase.value = "drill";
    beginAttempt();
  }

  /** 记录一次击键；首次击键时启动计时。 */
  function typeKey(key: string): void {
    if (phase.value !== "drill") return;
    const current = attempt.value;
    if (!current || current.finished) return;
    if (startedAt.value === null) {
      startedAt.value = Date.now();
      clock.value = startedAt.value;
      startClock();
    }
    attempt.value = typeCharacter(current, key);
  }

  /** 当前焦点位置是否本身就是空格字符（短语型单词）：是则空格按字符消费。 */
  function isSpaceLiteralHere(): boolean {
    return attempt.value !== null && !attempt.value.finished && isSpaceLiteral(attempt.value);
  }

  /** 每完成一个词，后台补一个还没缓存的词：按词池顺序找第一个缺口。 */
  function cacheNextWord(): void {
    for (const word of words.value) {
      if (!hasCachedAudio(word.word)) {
        cacheAudioInBackground(word.word);
        return;
      }
    }
  }

  /** 结束当前单词并推进到下一格或翻页；有缓存则顺带朗读一遍。 */
  function finishWord(): void {
    const item = activeItem.value;
    const current = attempt.value;
    if (phase.value !== "drill" || !item || !current || current.finished) return;
    const finished = finishAttempt(current);
    const summary: AttemptSummary = summarizeAttempt(finished);
    attempt.value = finished;
    records[item.id] = { marks: finished.marks.slice(), typed: finished.typed.slice(), summary };
    combo.value = nextCombo(summary, combo.value);
    maxCombo.value = Math.max(maxCombo.value, combo.value);
    score.value += scoreGain(summary, combo.value, item.memory);
    totals.correct += summary.correct;
    totals.wrong += summary.wrong;
    totals.missed += summary.missed;
    totals.wordsDone += 1;
    if (summary.perfect) totals.perfectWords += 1;
    playCachedAudio(item.word.word);
    cacheNextWord();
    advance();
  }

  /** 推进到本页下一格；本页打满就直接翻页，最后一页则结算。 */
  function advance(): void {
    const page = activePage.value;
    if (!page) return;
    if (itemIndex.value + 1 < page.items.length) {
      itemIndex.value += 1;
      beginAttempt();
      return;
    }
    // 本页打满：不再停在「本页完成」，直接进入下一页；最后一页直接结算。
    if (pageIndex.value + 1 < pages.value.length) {
      pageIndex.value += 1;
      itemIndex.value = 0;
      phase.value = "drill";
      beginAttempt();
      return;
    }
    phase.value = "finished";
    stopClock();
  }

  /** 再来一轮：同一词池与设置，成绩清零。 */
  function restart(): void {
    start();
  }

  /** 暂停 / 恢复：被设置或命令面板遮挡时不响应按键，也不让速度继续掉。 */
  function setActive(active: boolean): void {
    if (active) {
      if (pausedAt.value !== null) {
        pausedTotal.value += Math.max(0, Date.now() - pausedAt.value);
        pausedAt.value = null;
        clock.value = Date.now();
      }
      if (phase.value === "drill" && startedAt.value !== null) startClock();
      return;
    }
    if (startedAt.value !== null && pausedAt.value === null) {
      pausedAt.value = Date.now();
      clock.value = pausedAt.value;
    }
    stopClock();
  }

  /** 关闭工作区时释放定时器，防止过场回调在卸载后继续推进。 */
  function dispose(): void {
    disposed = true;
    stopClock();
  }

  return {
    phase,
    errorMessage,
    scope,
    words,
    pages,
    pageIndex,
    itemIndex,
    records,
    attempt,
    combo,
    maxCombo,
    score,
    totals,
    activePage,
    activeItem,
    doneCount,
    memoryMode,
    elapsedMs,
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
  };
}

/** 会话实例类型，供组件按用例拆分 props。 */
export type TypingSession = ReturnType<typeof createTypingSession>;
