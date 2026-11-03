import { ref, type Ref } from "vue";
import * as backend from "../api/backend";
import type { DictionaryEntry } from "../domain/types";

/**
 * 音标查询状态：idle 未查询 / loading 查询中 / ready 已查到音标 /
 * missing 词典未收录 / error 查询失败（静默，允许手填）。
 */
export type PhoneticLookupStatus = "idle" | "loading" | "ready" | "missing" | "error";

/** 默认防抖：用户停手 500ms 才查，避免每敲一个字母都打一次词典。 */
const DEFAULT_DELAY_MS = 500;

/** 查询实现：默认走 api 层的 ECDICT；组件测试注入内存表。 */
export type PhoneticLookupFn = (word: string) => Promise<DictionaryEntry | null>;

export interface PhoneticLookupOptions {
  /** 查询实现；缺省使用后端 lookup_dictionary_word。 */
  lookup?: PhoneticLookupFn;
  /** 防抖时长，测试可缩短；生产默认 500ms。 */
  delayMs?: number;
  /** 新结果回调：只有未被更晚请求取代的结果才会触发，自动填音标的策略留给组件。 */
  onResult?: (entry: DictionaryEntry | null) => void;
}

/** 音标查询用例：防抖、组合输入跳过、过期响应丢弃与状态机。 */
export interface PhoneticLookup {
  status: Ref<PhoneticLookupStatus>;
  entry: Ref<DictionaryEntry | null>;
  /** 输入变化：组合输入未确认时取消待查；空输入直接回到 idle。 */
  input(word: string, composing?: boolean): void;
  /** 立即查询（回车、失焦、按钮），跳过防抖但仍丢弃被取代的响应。 */
  lookupNow(word: string): Promise<DictionaryEntry | null>;
  /** 换页或换词时清空状态并取消待查。 */
  reset(): void;
  /** 卸载时清理计时器与在途结果。 */
  dispose(): void;
}

/**
 * 单词音标查询。
 *
 * 关键约束：
 * - 500ms 防抖，避免逐字母请求；组合输入（中文输入法）期间绝不查。
 * - 请求号随每次查询自增，迟到响应直接丢弃，防止把 "abc" 的音标填到 "abcd"。
 * - 词典失败只把状态置为 error，不抛给页面：音标是辅助信息，永远可以手填。
 */
export function usePhoneticLookup(options: PhoneticLookupOptions = {}): PhoneticLookup {
  const lookup: PhoneticLookupFn = options.lookup ?? ((word: string) => backend.lookupDictionaryWord(word));
  const delayMs = options.delayMs ?? DEFAULT_DELAY_MS;
  const status = ref<PhoneticLookupStatus>("idle");
  const entry = ref<DictionaryEntry | null>(null);
  let timer: ReturnType<typeof setTimeout> | null = null;
  let requestId = 0;
  let disposed = false;
  /** 最近一次真正发起查询的词，用于同一词不重复请求。 */
  let lastWord = "";

  /** 取消待发查询；在途请求由自增请求号失效。 */
  function cancelPending(): void {
    if (timer !== null) {
      clearTimeout(timer);
      timer = null;
    }
    requestId += 1;
  }

  /** 清空展示状态并取消一切在途工作。 */
  function reset(): void {
    cancelPending();
    entry.value = null;
    status.value = "idle";
    lastWord = "";
  }

  /** 执行一次查询；只有最新请求可以写状态。 */
  async function run(word: string): Promise<DictionaryEntry | null> {
    const target = word.trim();
    if (!target) {
      reset();
      return null;
    }
    cancelPending();
    const id = requestId;
    lastWord = target;
    status.value = "loading";
    try {
      const result = await lookup(target);
      if (disposed || id !== requestId) return null;
      entry.value = result;
      status.value = (result?.phonetic ?? "").trim() ? "ready" : "missing";
      options.onResult?.(result);
      return result;
    } catch {
      if (disposed || id !== requestId) return null;
      entry.value = null;
      status.value = "error";
      options.onResult?.(null);
      return null;
    }
  }

  /** 输入变化入口：组合输入只取消待查，不查询也不改状态。 */
  function input(word: string, composing = false): void {
    if (disposed) return;
    const target = word.trim();
    if (composing) {
      cancelPending();
      return;
    }
    cancelPending();
    if (!target) {
      entry.value = null;
      status.value = "idle";
      lastWord = "";
      return;
    }
    // 同一个词已经有结果时不必再查（改错一个字母再改回来也不浪费一次请求）。
    if (target === lastWord && status.value === "ready") return;
    timer = setTimeout(() => {
      timer = null;
      void run(target);
    }, delayMs);
  }

  /** 立即查询：按钮、回车与失焦都走这里。 */
  function lookupNow(word: string): Promise<DictionaryEntry | null> {
    if (disposed) return Promise.resolve(null);
    if (timer !== null) {
      clearTimeout(timer);
      timer = null;
    }
    return run(word);
  }

  /** 卸载后迟到响应不得再写状态。 */
  function dispose(): void {
    disposed = true;
    reset();
  }

  return { status, entry, input, lookupNow, reset, dispose };
}
