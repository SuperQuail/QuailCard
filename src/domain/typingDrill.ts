import type { ReviewCard } from "./types";

/**
 * 打字背诵的纯领域规则：词池、练习项、分页、逐字母判定与得分。
 *
 * 契约：本模块不依赖 Vue、不调用后端、不读写存储，全部输入输出都是普通值，
 * 便于单测，也便于会话层在不关心渲染的情况下推进状态。
 */

/** 打字背诵使用的单词快照（来自单词卡）。 */
export interface TypingWord {
  cardId: string;
  notePath: string;
  /** 中文释义（卡片 front）。 */
  meaning: string;
  /** 英文原词（卡片 back），已压缩连续空白。 */
  word: string;
  /** 音标（卡片 detail），可为空。 */
  phonetic: string;
  /** 例句（卡片 example），可为空。 */
  example: string;
}

/** 页面上的一个练习项：同一个单词的一次作答。 */
export interface TypingItem {
  id: string;
  word: TypingWord;
  /** 第几遍，从 1 开始；记忆遍等于 repeatsPerWord + 1。 */
  pass: number;
  /** 是否为最后的记忆遍：释义、原词与本页已完成的记录都会被模糊。 */
  memory: boolean;
}

/** 一页练习：按整词切分，绝不把同一个单词拆到两页。 */
export interface TypingPage {
  index: number;
  items: TypingItem[];
}

/** 单个字母的判定状态。 */
export type LetterMark = "pending" | "correct" | "wrong" | "missed";

/** 用户实际打出的字符：记忆遍在目标模糊时据此提供红绿反馈。 */
export interface TypedLetter {
  char: string;
  correct: boolean;
}

/** 一次作答的进行中状态。 */
export interface TypingAttempt {
  itemId: string;
  /** 目标字母，按码点拆分，保证代理对与 emoji 不被拆开。 */
  chars: string[];
  marks: LetterMark[];
  typed: TypedLetter[];
  cursor: number;
  finished: boolean;
}

/** 一次作答的结算摘要。 */
export interface AttemptSummary {
  perfect: boolean;
  correct: number;
  wrong: number;
  missed: number;
}

/** 已完成的作答记录，用于「前面的记录不清」。 */
export interface TypingRecord {
  marks: LetterMark[];
  typed: TypedLetter[];
  summary: AttemptSummary;
}

/**
 * 把输入键归一化为单个字符。
 * 契约：全角字母数字转半角、全角空格转半角空格，其余原样返回；
 * 多字符（组合输入、功能键）返回空串，由调用方忽略。
 */
export function normalizeKey(key: string): string {
  const points = [...key];
  if (points.length !== 1) {
    return "";
  }
  const code = points[0].codePointAt(0) ?? 0;
  if (code === 0x3000) {
    return " ";
  }
  if (code >= 0xff01 && code <= 0xff5e) {
    return String.fromCodePoint(code - 0xfee0);
  }
  return points[0];
}

/**
 * 目标词的规范形式：去首尾空白并把连续空白压成一个空格。
 * 契约：空格在词内是普通字符，因此必须先把空白形态固定下来再拆分。
 */
export function normalizeWord(word: string): string {
  return word.trim().replace(/\s+/g, " ");
}

/** 字母比较：忽略大小写，其余按原样比较。 */
function sameLetter(target: string, input: string): boolean {
  return target.toLowerCase() === input.toLowerCase();
}

/**
 * 从复习队列快照里挑出可练习的单词。
 * 契约：只收单词卡；中英任一为空则跳过；按英文小写去重并保留队列顺序，
 * 避免同一拼写在不同笔记里被练两遍。
 */
export function buildTypingWords(cards: ReviewCard[]): TypingWord[] {
  const seen = new Set<string>();
  const words: TypingWord[] = [];
  for (const card of cards) {
    if (card.kind !== "vocabulary") {
      continue;
    }
    const meaning = card.front.trim();
    const word = normalizeWord(card.back);
    if (!meaning || !word) {
      continue;
    }
    const key = word.toLowerCase();
    if (seen.has(key)) {
      continue;
    }
    seen.add(key);
    words.push({
      cardId: card.id,
      notePath: card.notePath,
      meaning,
      word,
      phonetic: card.detail.trim(),
      example: card.example.trim(),
    });
  }
  return words;
}

/**
 * 把单词切成练习页。
 * 契约：每页 wordsPerPage 个整词；每个单词生成 repeatsPerWord 个普通项 +
 * 1 个记忆项；参数先取整并夹到至少 1，坏配置只会退化成 1，不会生成空页。
 */
export function buildTypingPages(words: TypingWord[], repeatsPerWord: number, wordsPerPage: number): TypingPage[] {
  const repeats = Math.max(1, Math.trunc(repeatsPerWord) || 1);
  const perPage = Math.max(1, Math.trunc(wordsPerPage) || 1);
  const pages: TypingPage[] = [];
  for (let start = 0, index = 0; start < words.length; start += perPage, index += 1) {
    const items: TypingItem[] = [];
    for (const word of words.slice(start, start + perPage)) {
      for (let pass = 1; pass <= repeats; pass += 1) {
        items.push({ id: `${word.cardId}:${pass}`, word, pass, memory: false });
      }
      items.push({ id: `${word.cardId}:memory`, word, pass: repeats + 1, memory: true });
    }
    pages.push({ index, items });
  }
  return pages;
}

/** 为一次作答建立初始状态：目标字母全部待打，光标在开头。 */
export function startAttempt(item: TypingItem): TypingAttempt {
  const chars = [...item.word.word];
  return {
    itemId: item.id,
    chars,
    marks: chars.map(() => "pending" as LetterMark),
    typed: [],
    cursor: 0,
    finished: false,
  };
}

/**
 * 记录一次击键。
 * 契约：只前进不回退（背诵场景不允许改错），到词尾后的多余击键直接忽略；
 * 无法归一化为单字符的键（组合输入、功能键）不改变状态。
 */
export function typeCharacter(attempt: TypingAttempt, key: string): TypingAttempt {
  if (attempt.finished) {
    return attempt;
  }
  const char = normalizeKey(key);
  if (!char || attempt.cursor >= attempt.chars.length) {
    return attempt;
  }
  const correct = sameLetter(attempt.chars[attempt.cursor], char);
  const marks = attempt.marks.slice();
  marks[attempt.cursor] = correct ? "correct" : "wrong";
  return {
    ...attempt,
    marks,
    typed: [...attempt.typed, { char, correct }],
    cursor: attempt.cursor + 1,
  };
}

/** 当前位置是否本身就是一个空格字符（短语型单词）：是则空格按字符消费而不是结束单词。 */
export function isSpaceLiteral(attempt: TypingAttempt): boolean {
  return attempt.chars[attempt.cursor] === " ";
}

/** 结束当前单词：未打出的位置记为漏打，已打错的保持红色。 */
export function finishAttempt(attempt: TypingAttempt): TypingAttempt {
  if (attempt.finished) {
    return attempt;
  }
  return {
    ...attempt,
    marks: attempt.marks.map((mark) => (mark === "pending" ? "missed" : mark)),
    finished: true,
  };
}

/** 统计一次作答的对错数量；只有全绿才算完美。 */
export function summarizeAttempt(attempt: TypingAttempt): AttemptSummary {
  let correct = 0;
  let wrong = 0;
  let missed = 0;
  for (const mark of attempt.marks) {
    if (mark === "correct") {
      correct += 1;
    } else if (mark === "wrong") {
      wrong += 1;
    } else if (mark === "missed") {
      missed += 1;
    }
  }
  return { perfect: correct === attempt.marks.length && wrong === 0 && missed === 0, correct, wrong, missed };
}

/** 连击推进：完美词 +1，否则清零。 */
export function nextCombo(summary: AttemptSummary, combo: number): number {
  return summary.perfect ? combo + 1 : 0;
}

/** 完美词得分：基础 10 分 + 连击加成（最多 10）+ 记忆遍额外 10 分；打错不得分。 */
export function scoreGain(summary: AttemptSummary, combo: number, memory: boolean): number {
  if (!summary.perfect) {
    return 0;
  }
  return 10 + Math.min(Math.max(combo, 1), 10) + (memory ? 10 : 0);
}

/** 命中率：没有任何击键时按 100% 处理，避免除零与误导性的 0%。 */
export function accuracyOf(totals: { correct: number; wrong: number; missed: number }): number {
  const total = totals.correct + totals.wrong + totals.missed;
  return total === 0 ? 1 : totals.correct / total;
}
