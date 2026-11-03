import { beforeEach, describe, expect, test, vi } from "vitest";
import * as backend from "../api/backend";
import { createTypingSession, PRELOAD_WORD_COUNT } from "./typingSession";
import { resetAudioCache } from "./typingAudio";
import type { ReviewCard } from "../domain/types";

vi.mock("../api/backend", () => ({
  getReviewQueue: vi.fn(),
  submitReview: vi.fn(),
  getStudyStats: vi.fn(),
  synthesizeSpeech: vi.fn(),
}));

/** 构造单词卡，字段与后端队列一致。 */
function card(id: string, front: string, back: string): ReviewCard {
  return {
    id,
    notePath: "英语/词汇.md",
    sourceRef: "",
    kind: "vocabulary",
    front,
    back,
    detail: "",
    example: "",
    aliases: [],
    rubricPoints: [],
    state: "new",
    version: 1,
  };
}

const cards = [card("c1", "短暂的", "ephemeral"), card("c2", "深入思考", "contemplate")];
/** 12 个词：用来验证「只预取前 10 个、之后每完成一个补一个」。 */
const manyCards = Array.from({ length: 12 }, (_, index) => card(`m${index}`, `释义 ${index}`, `word${index}`));

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

/** 建一个每词 1 遍、每页 2 词的会话：每页 4 格，正好一页。 */
function session(scope: "note" | "all" = "all") {
  return createTypingSession({
    notePath: scope === "note" ? "英语/词汇.md" : null,
    scope,
    settings: () => ({ repeatsPerWord: 1, wordsPerPage: 2 }),
  });
}

beforeEach(() => {
  vi.clearAllMocks();
  vi.stubGlobal("Audio", FakeAudio);
  played.length = 0;
  resetAudioCache();
  vi.mocked(backend.getReviewQueue).mockResolvedValue(cards);
  vi.mocked(backend.synthesizeSpeech).mockResolvedValue("data:audio/mp3;base64,AAAA");
});

describe("打字背诵会话", () => {
  test("加载后进入准备态，全部范围按 null + includeAll 取词", async () => {
    const flow = session();
    await flow.load();
    expect(backend.getReviewQueue).toHaveBeenCalledWith(null, true);
    expect(flow.phase.value).toBe("setup");
    expect(flow.pages.value).toHaveLength(1);
    expect(flow.pages.value[0].items).toHaveLength(4);
  });

  test("笔记范围把路径传给后端", async () => {
    const flow = session("note");
    await flow.load();
    expect(backend.getReviewQueue).toHaveBeenCalledWith("英语/词汇.md", true);
  });

  test("进入前只预取前 N 个单词的发音，完成后才允许开始", async () => {
    vi.mocked(backend.getReviewQueue).mockResolvedValue(manyCards);
    const flow = session();
    await flow.load();
    expect(backend.synthesizeSpeech).toHaveBeenCalledTimes(PRELOAD_WORD_COUNT);
    expect(flow.audioReady.value).toBe(true);
    expect(flow.audioProgress.done).toBe(PRELOAD_WORD_COUNT);
  });

  test("发音没就绪时开始按钮不生效", async () => {
    const releases: Array<(value: string) => void> = [];
    vi.mocked(backend.synthesizeSpeech).mockImplementation(() => new Promise<string>((resolve) => { releases.push(resolve); }));
    const flow = session();
    void flow.load();
    await vi.waitFor(() => expect(flow.phase.value).toBe("setup"));
    expect(flow.audioReady.value).toBe(false);
    flow.start();
    expect(flow.phase.value).not.toBe("drill");
    for (const release of releases) release("data:audio/mp3;base64,AAAA");
    await vi.waitFor(() => expect(flow.audioReady.value).toBe(true));
    flow.start();
    expect(flow.phase.value).toBe("drill");
  });

  test("击键推进当前格，空格收尾后更新连击、得分与记录", async () => {
    const flow = session();
    await flow.load();
    flow.start();
    expect(flow.phase.value).toBe("drill");
    for (const char of "ephemeral") flow.typeKey(char);
    expect(flow.attempt.value?.marks.every((mark) => mark === "correct")).toBe(true);
    flow.finishWord();
    expect(flow.combo.value).toBe(1);
    expect(flow.maxCombo.value).toBe(1);
    expect(flow.score.value).toBe(11);
    expect(flow.records["c1:1"]?.summary.perfect).toBe(true);
    expect(flow.itemIndex.value).toBe(1);
    expect(flow.attempt.value?.itemId).toBe("c1:memory");
  });

  test("完成一个词后播放已缓存发音，并补缓存下一个未缓存的词", async () => {
    vi.mocked(backend.getReviewQueue).mockResolvedValue(manyCards);
    const flow = session();
    await flow.load();
    flow.start();
    flow.finishWord();
    expect(played).toHaveLength(1);
    expect(played[0]).toBe("data:audio/mp3;base64,AAAA");
    // 前 10 个已预取；完成一个词后后台补第 11 个
    await vi.waitFor(() => expect(backend.synthesizeSpeech).toHaveBeenCalledTimes(PRELOAD_WORD_COUNT + 1));
    expect(backend.synthesizeSpeech).toHaveBeenLastCalledWith("word10");
  });

  test("打错清零连击，但与会话统计一起保留红字与漏打", async () => {
    const flow = session();
    await flow.load();
    flow.start();
    flow.typeKey("e");
    flow.typeKey("x");
    flow.finishWord();
    expect(flow.combo.value).toBe(0);
    expect(flow.score.value).toBe(0);
    expect(flow.records["c1:1"]?.summary).toEqual({ perfect: false, correct: 1, wrong: 1, missed: 7 });
    expect(flow.totals.wrong).toBe(1);
  });

  test("记忆遍只在记忆项为真，完成后进入下一词", async () => {
    const flow = session();
    await flow.load();
    flow.start();
    for (const char of "ephemeral") flow.typeKey(char);
    flow.finishWord();
    expect(flow.memoryMode.value).toBe(true);
    for (const char of "ephemeral") flow.typeKey(char);
    flow.finishWord();
    expect(flow.memoryMode.value).toBe(false);
    expect(flow.attempt.value?.itemId).toBe("c2:1");
  });

  test("本页打满直接翻页，最后一页打满直接结算", async () => {
    vi.mocked(backend.getReviewQueue).mockResolvedValue([
      card("a", "一", "alpha"),
      card("b", "二", "beta"),
      card("c", "三", "gamma"),
      card("d", "四", "delta"),
    ]);
    const flow = session();
    await flow.load();
    flow.start();
    for (let index = 0; index < 4; index += 1) {
      flow.finishWord();
    }
    expect(flow.phase.value).toBe("drill");
    expect(flow.pageIndex.value).toBe(1);
    for (let index = 0; index < 4; index += 1) {
      flow.finishWord();
    }
    expect(flow.phase.value).toBe("finished");
  });

  test("全程不提交评分，也不读学习统计", async () => {
    const flow = session();
    await flow.load();
    flow.start();
    for (let index = 0; index < 4; index += 1) {
      flow.finishWord();
    }
    expect(backend.submitReview).not.toHaveBeenCalled();
    expect(backend.getStudyStats).not.toHaveBeenCalled();
  });

  test("空词池进入空态，加载失败进入错误态", async () => {
    vi.mocked(backend.getReviewQueue).mockResolvedValueOnce([{ ...card("q1", "问题", "参考内容"), kind: "qa" }]);
    const empty = session();
    await empty.load();
    expect(empty.phase.value).toBe("empty");

    vi.mocked(backend.getReviewQueue).mockRejectedValueOnce(new Error("读取失败"));
    const failed = session();
    await failed.load();
    expect(failed.phase.value).toBe("error");
    expect(failed.errorMessage.value).toBe("读取失败");
  });

  test("切回已加载过的范围不再重新拉取词池", async () => {
    vi.mocked(backend.getReviewQueue).mockImplementation(async (path) => path ? cards : manyCards);
    const flow = session("note");
    await flow.load();
    await flow.setScope("all");
    expect(backend.getReviewQueue).toHaveBeenCalledTimes(2);
    await flow.setScope("note");
    expect(backend.getReviewQueue).toHaveBeenCalledTimes(2);
    expect(flow.words.value).toHaveLength(2);
  });

  test("暂停后计时不再前进，恢复后继续累计", async () => {
    vi.useFakeTimers();
    try {
      const flow = session();
      await flow.load();
      flow.start();
      flow.typeKey("e");
      vi.advanceTimersByTime(1000);
      const beforePause = flow.elapsedMs.value;
      expect(beforePause).toBeGreaterThan(0);
      flow.setActive(false);
      vi.advanceTimersByTime(5000);
      expect(flow.elapsedMs.value).toBe(beforePause);
      flow.setActive(true);
      vi.advanceTimersByTime(1000);
      expect(flow.elapsedMs.value).toBeGreaterThan(beforePause);
      flow.dispose();
    } finally {
      vi.useRealTimers();
    }
  });
});
