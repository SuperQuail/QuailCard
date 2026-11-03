import { afterEach, beforeEach, describe, expect, test, vi } from "vitest";
import { usePhoneticLookup } from "./usePhoneticLookup";
import type { DictionaryEntry } from "../domain/types";

/** 构造词典条目：除 word 外字段可空，与后端 DictionaryEntry 一致。 */
function dictionaryEntry(word: string, phonetic: string | null = "/test/"): DictionaryEntry {
  return { word, phonetic, translation: null, definition: null, pos: null, collins: null, oxford: null, bnc: null, frq: null, exchange: null };
}

beforeEach(() => {
  vi.useFakeTimers();
});

afterEach(() => {
  vi.useRealTimers();
});

describe("音标查询防抖", () => {
  test("输入停 500ms 才查询，期间的输入取消上一次等待", async () => {
    const lookup = vi.fn(async (word: string) => dictionaryEntry(word));
    const state = usePhoneticLookup({ lookup });
    state.input("e");
    await vi.advanceTimersByTimeAsync(400);
    state.input("ephemeral");
    await vi.advanceTimersByTimeAsync(499);
    expect(lookup).not.toHaveBeenCalled();
    await vi.advanceTimersByTimeAsync(1);
    expect(lookup).toHaveBeenCalledTimes(1);
    expect(lookup).toHaveBeenCalledWith("ephemeral");
    expect(state.status.value).toBe("ready");
    expect(state.entry.value?.phonetic).toBe("/test/");
  });

  test("组合输入期间不查询，候选确认后补查", async () => {
    const lookup = vi.fn(async (word: string) => dictionaryEntry(word));
    const state = usePhoneticLookup({ lookup });
    state.input("ephemeral", true);
    await vi.advanceTimersByTimeAsync(1000);
    expect(lookup).not.toHaveBeenCalled();
    state.input("ephemeral", false);
    await vi.advanceTimersByTimeAsync(500);
    expect(lookup).toHaveBeenCalledWith("ephemeral");
  });

  test("空输入回到 idle，不发起查询", async () => {
    const lookup = vi.fn(async (word: string) => dictionaryEntry(word));
    const state = usePhoneticLookup({ lookup });
    state.input("abc");
    state.input("   ");
    await vi.advanceTimersByTimeAsync(1000);
    expect(lookup).not.toHaveBeenCalled();
    expect(state.status.value).toBe("idle");
    expect(state.entry.value).toBeNull();
  });
});

describe("音标查询结果", () => {
  test("立即查询跳过防抖；未收录落到 missing，失败落到 error 并回调", async () => {
    const onResult = vi.fn();
    const lookup = vi.fn(async () => null);
    const state = usePhoneticLookup({ lookup, onResult });
    await state.lookupNow("unknown");
    expect(state.status.value).toBe("missing");
    expect(onResult).toHaveBeenLastCalledWith(null);

    lookup.mockRejectedValueOnce(new Error("词典不可用"));
    await state.lookupNow("boom");
    expect(state.status.value).toBe("error");
    expect(state.entry.value).toBeNull();
  });

  test("过期响应丢弃：后发请求先返回时旧结果不覆盖", async () => {
    let resolveFirst!: (value: DictionaryEntry | null) => void;
    const lookup = vi.fn()
      .mockImplementationOnce(() => new Promise<DictionaryEntry | null>((resolve) => { resolveFirst = resolve; }))
      .mockImplementationOnce(async () => dictionaryEntry("second", "/second/"));
    const state = usePhoneticLookup({ lookup });
    const first = state.lookupNow("first");
    await state.lookupNow("second");
    resolveFirst(dictionaryEntry("first", "/first/"));
    await first;
    expect(state.entry.value?.phonetic).toBe("/second/");
    expect(state.status.value).toBe("ready");
  });

  test("dispose 后迟到响应不再写状态", async () => {
    let resolveLookup!: (value: DictionaryEntry | null) => void;
    const lookup = vi.fn(() => new Promise<DictionaryEntry | null>((resolve) => { resolveLookup = resolve; }));
    const state = usePhoneticLookup({ lookup });
    const pending = state.lookupNow("late");
    state.dispose();
    resolveLookup(dictionaryEntry("late", "/late/"));
    await pending;
    expect(state.entry.value).toBeNull();
    expect(state.status.value).toBe("idle");
  });

  test("reset 清空状态并取消待发查询", async () => {
    const lookup = vi.fn(async (word: string) => dictionaryEntry(word));
    const state = usePhoneticLookup({ lookup });
    state.input("abc");
    state.reset();
    await vi.advanceTimersByTimeAsync(1000);
    expect(lookup).not.toHaveBeenCalled();
    expect(state.status.value).toBe("idle");
  });
});
