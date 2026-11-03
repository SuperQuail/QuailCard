import { describe, expect, test } from "vitest";
import {
  accuracyOf,
  buildTypingPages,
  buildTypingWords,
  finishAttempt,
  isSpaceLiteral,
  nextCombo,
  normalizeKey,
  normalizeWord,
  scoreGain,
  startAttempt,
  summarizeAttempt,
  typeCharacter,
  type TypingItem,
} from "./typingDrill";
import type { ReviewCard } from "./types";

/** 构造一张单词卡，只覆盖测试关心的字段。 */
function card(overrides: Partial<ReviewCard> = {}): ReviewCard {
  return {
    id: "card-ephemeral",
    notePath: "英语/词汇.md",
    sourceRef: "",
    kind: "vocabulary",
    front: "短暂的；转瞬即逝的",
    back: "ephemeral",
    detail: "/ɪˈfemərəl/",
    example: "Fame is ephemeral.",
    aliases: ["ephemerally"],
    rubricPoints: [],
    state: "new",
    version: 1,
    ...overrides,
  };
}

/** 从页面里取出第一项，供击键类用例复用。 */
function firstItem(words = [card()]): TypingItem {
  return buildTypingPages(buildTypingWords(words), 5, 3)[0].items[0];
}

/** 依次打出一串字符，模拟不允许回退的连续击键。 */
function typeAll(item: TypingItem, keys: string) {
  return [...keys].reduce((attempt, key) => typeCharacter(attempt, key), startAttempt(item));
}

describe("打字背诵领域规则", () => {
  test("只保留单词卡、丢弃空字段并按小写词形去重", () => {
    const words = buildTypingWords([
      card(),
      card({ id: "card-qa", kind: "qa" }),
      card({ id: "card-empty", front: "  " }),
      card({ id: "card-dup", notePath: "另一篇.md", front: "短暂" }),
      card({ id: "card-contemplate", back: "contemplate", front: "深入思考" }),
    ]);
    expect(words.map((word) => word.word)).toEqual(["ephemeral", "contemplate"]);
    expect(words[0].meaning).toBe("短暂的；转瞬即逝的");
    expect(words[0].phonetic).toBe("/ɪˈfemərəl/");
  });

  test("每词生成 N 个普通项和一个记忆项，分页按整词切分", () => {
    const words = buildTypingWords([
      card(),
      card({ id: "card-2", back: "resilient", front: "有复原力的" }),
      card({ id: "card-3", back: "ubiquitous", front: "无处不在的" }),
      card({ id: "card-4", back: "meticulous", front: "一丝不苟的" }),
    ]);
    const pages = buildTypingPages(words, 5, 3);
    expect(pages).toHaveLength(2);
    expect(pages[0].items).toHaveLength(18);
    expect(pages[1].items).toHaveLength(6);
    expect(pages[0].items[5].memory).toBe(true);
    expect(pages[0].items[5].pass).toBe(6);
    expect(pages[0].items[0].memory).toBe(false);
    expect(new Set(pages[0].items.map((item) => item.id)).size).toBe(18);
  });

  test("非法的遍数或每页词数退化为 1，而不是生成空页", () => {
    const pages = buildTypingPages(buildTypingWords([card()]), 0, 0);
    expect(pages).toHaveLength(1);
    expect(pages[0].items).toHaveLength(2);
  });

  test("正确键变绿、错误键变红，光标只前进不回退", () => {
    const item = firstItem();
    const attempt = typeAll(item, "eph");
    expect(attempt.cursor).toBe(3);
    expect(attempt.marks.slice(0, 3)).toEqual(["correct", "correct", "correct"]);
    const wrong = typeCharacter(attempt, "x");
    expect(wrong.cursor).toBe(4);
    expect(wrong.marks[3]).toBe("wrong");
    expect(wrong.typed[3]).toEqual({ char: "x", correct: false });
  });

  test("大小写与全角字母都按同一个字母处理", () => {
    const item = firstItem();
    const upper = typeCharacter(startAttempt(item), "E");
    expect(upper.marks[0]).toBe("correct");
    expect(normalizeKey("Ｅ")).toBe("E");
    const fullWidth = typeCharacter(startAttempt(item), "Ｅ");
    expect(fullWidth.marks[0]).toBe("correct");
  });

  test("结束单词把未打位置记为漏打，打错的保持红色", () => {
    const item = firstItem();
    const attempt = typeAll(item, "epx");
    const finished = finishAttempt(attempt);
    expect(finished.finished).toBe(true);
    expect(finished.marks).toEqual([
      "correct",
      "correct",
      "wrong",
      "missed",
      "missed",
      "missed",
      "missed",
      "missed",
      "missed",
    ]);
  });

  test("全绿才算完美，任何红字或漏打都不算", () => {
    const item = firstItem();
    expect(summarizeAttempt(finishAttempt(typeAll(item, "ephemeral")))).toEqual({ perfect: true, correct: 9, wrong: 0, missed: 0 });
    expect(summarizeAttempt(finishAttempt(typeAll(item, "ephemerxl"))).perfect).toBe(false);
    expect(summarizeAttempt(finishAttempt(typeAll(item, "ephem"))).perfect).toBe(false);
  });

  test("短语里的空格按普通字符消费，其余位置的空格用于结束单词", () => {
    const item = buildTypingPages(buildTypingWords([card({ back: "make up", front: "编造" })]), 1, 1)[0].items[0];
    const attempt = typeAll(item, "make");
    expect(isSpaceLiteral(attempt)).toBe(true);
    const withSpace = typeCharacter(attempt, " ");
    expect(withSpace.marks[4]).toBe("correct");
    expect(isSpaceLiteral(withSpace)).toBe(false);
  });

  test("词尾之后的多余击键被忽略，也不产生新的红字", () => {
    const item = firstItem();
    const attempt = typeAll(item, "ephemeralxyz");
    expect(attempt.cursor).toBe(9);
    expect(attempt.marks.every((mark) => mark === "correct")).toBe(true);
  });

  test("连击完美加一、打错清零，得分按连击与记忆遍加成计算", () => {
    const perfect = { perfect: true, correct: 9, wrong: 0, missed: 0 };
    const wrong = { perfect: false, correct: 8, wrong: 1, missed: 0 };
    expect(nextCombo(perfect, 2)).toBe(3);
    expect(nextCombo(wrong, 3)).toBe(0);
    expect(scoreGain(perfect, 1, false)).toBe(11);
    expect(scoreGain(perfect, 20, false)).toBe(20);
    expect(scoreGain(perfect, 5, true)).toBe(25);
    expect(scoreGain(wrong, 5, false)).toBe(0);
  });

  test("命中率在没有击键时按 100% 处理", () => {
    expect(accuracyOf({ correct: 0, wrong: 0, missed: 0 })).toBe(1);
    expect(accuracyOf({ correct: 3, wrong: 1, missed: 0 })).toBeCloseTo(0.75);
  });

  test("目标词规范化：去首尾空白并压缩内部空白", () => {
    expect(normalizeWord("  make   up  ")).toBe("make up");
  });
});
