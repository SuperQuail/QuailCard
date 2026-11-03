import { afterEach, beforeEach, describe, expect, test, vi } from "vitest";
import * as backend from "../api/backend";
import {
  cacheAudioInBackground,
  cachedAudioCount,
  hasCachedAudio,
  playCachedAudio,
  preloadAudio,
  resetAudioCache,
} from "./typingAudio";

vi.mock("../api/backend", () => ({ synthesizeSpeech: vi.fn() }));

/** 记录播放过的音频地址，避免 jsdom 真正播放。 */
const played: string[] = [];
class FakeAudio {
  src: string;
  constructor(src: string) {
    this.src = src;
    played.push(src);
  }
  pause(): void {}
  play(): Promise<void> {
    return Promise.resolve();
  }
}

/** 记录系统语音朗读过的单词。 */
const spoken: string[] = [];
class FakeUtterance {
  text: string;
  lang = "";
  rate = 1;
  constructor(text: string) {
    this.text = text;
  }
}

afterEach(() => {
  vi.unstubAllGlobals();
});

beforeEach(() => {
  vi.clearAllMocks();
  vi.stubGlobal("Audio", FakeAudio);
  vi.stubGlobal("SpeechSynthesisUtterance", FakeUtterance);
  vi.stubGlobal("speechSynthesis", { cancel: () => {}, speak: (utterance: FakeUtterance) => spoken.push(utterance.text) });
  played.length = 0;
  spoken.length = 0;
  resetAudioCache();
});

describe("打字背诵发音缓存", () => {
  test("并发预取并汇报进度，重复单词只合成一次", async () => {
    vi.mocked(backend.synthesizeSpeech).mockImplementation(async (word) => `data:${word}`);
    const progress: Array<[number, number]> = [];
    await preloadAudio(["a", "b", "a"], (done, total) => progress.push([done, total]));
    expect(backend.synthesizeSpeech).toHaveBeenCalledTimes(2);
    expect(hasCachedAudio("a")).toBe(true);
    expect(cachedAudioCount()).toBe(2);
    expect(progress[progress.length - 1]).toEqual([2, 2]);
  });

  test("后端合成失败时改用系统语音兜底，不打断其他词", async () => {
    vi.mocked(backend.synthesizeSpeech)
      .mockRejectedValueOnce(new Error("TTS_CONNECT_FAILED"))
      .mockResolvedValueOnce("data:ok");
    await preloadAudio(["bad", "good"]);
    expect(hasCachedAudio("bad")).toBe(true);
    expect(hasCachedAudio("good")).toBe(true);
    playCachedAudio("bad");
    expect(spoken).toEqual(["bad"]);
    playCachedAudio("good");
    expect(played).toEqual(["data:ok"]);
  });

  test("没准备过的词不播放也不朗读", () => {
    playCachedAudio("missing");
    expect(played).toHaveLength(0);
    expect(spoken).toHaveLength(0);
  });

  test("后台补缓存串行执行并去重", async () => {
    vi.mocked(backend.synthesizeSpeech).mockResolvedValue("data:ok");
    cacheAudioInBackground("later");
    cacheAudioInBackground("later");
    await vi.waitFor(() => expect(hasCachedAudio("later")).toBe(true));
    expect(backend.synthesizeSpeech).toHaveBeenCalledTimes(1);
  });

  test("重置后缓存清空", async () => {
    vi.mocked(backend.synthesizeSpeech).mockResolvedValue("data:ok");
    await preloadAudio(["word"]);
    resetAudioCache();
    expect(cachedAudioCount()).toBe(0);
    playCachedAudio("word");
    expect(played).toHaveLength(0);
    expect(spoken).toHaveLength(0);
  });
});
