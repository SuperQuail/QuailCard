import { describe, expect, test, vi } from "vitest";
import {
  createEntry,
  createEntryId,
  isEntryComplete,
  normalizePhonetic,
  parseWordGroup,
  serializeWordGroup,
  WORD_GROUP_FORMAT_VERSION,
  type WordEntry,
} from "./wordGroup";

/** 构造一页测试数据；只覆盖需要变化的字段。 */
function entry(overrides: Partial<WordEntry> = {}): WordEntry {
  return { id: "page-1", word: "ephemeral", phonetic: "/ɪˈfemərəl/", meaning: "短暂的", example: "Fame is ephemeral.", ...overrides };
}

describe("单词组解析", () => {
  /** 读容忍：缺字段取空串，未知字段忽略，缺失 formatVersion 按 1。 */
  test("合法文档按页返回，缺字段与未知字段都不影响解析", () => {
    const result = parseWordGroup(JSON.stringify({ words: [
      { id: "p1", word: "alpha", extra: "ignored" },
      { id: "p2", meaning: "贝塔", phonetic: null, example: null },
    ] }));
    expect(result.ok).toBe(true);
    if (!result.ok) return;
    expect(result.document.formatVersion).toBe(1);
    expect(result.document.words).toEqual([
      { id: "p1", word: "alpha", phonetic: "", meaning: "", example: "" },
      { id: "p2", word: "", phonetic: "", meaning: "贝塔", example: "" },
    ]);
  });

  /** 版本熔断：更高版本拒绝打开，避免旧应用覆盖新格式。 */
  test("更高版本拒绝解析并给出可展示原因", () => {
    const result = parseWordGroup(JSON.stringify({ formatVersion: WORD_GROUP_FORMAT_VERSION + 1, words: [] }));
    expect(result.ok).toBe(false);
    if (result.ok) return;
    expect(result.error).toContain("更新版本");
  });

  test("非法 JSON、顶层非对象、words 非数组、页 id 非法都拒绝解析", () => {
    for (const content of [
      "not json",
      "[]",
      JSON.stringify({ formatVersion: 1 }),
      JSON.stringify({ formatVersion: 1, words: ["x"] }),
      JSON.stringify({ formatVersion: 1, words: [{ word: "no-id" }] }),
      JSON.stringify({ formatVersion: 1, words: [{ id: "bad id!", word: "x" }] }),
      JSON.stringify({ formatVersion: 0, words: [] }),
    ]) {
      const result = parseWordGroup(content);
      expect(result.ok).toBe(false);
      if (result.ok) continue;
      expect(result.error.length).toBeGreaterThan(0);
    }
  });

  test("已知字段类型错误时拒绝解析，不静默改写用户内容", () => {
    const result = parseWordGroup(JSON.stringify({ formatVersion: 1, words: [{ id: "p1", word: 42 }] }));
    expect(result.ok).toBe(false);
  });
});

describe("单词组序列化", () => {
  /** 写完整：稳定字段序 + 2 空格缩进 + 末尾换行；同样内容永远得到同样字节。 */
  test("稳定字段序、2 空格缩进、末尾换行且可往返", () => {
    const content = serializeWordGroup({ formatVersion: 1, words: [entry()] });
    expect(content.startsWith('{\n  "formatVersion": 1,\n  "words": [\n    {\n      "id": "page-1",\n      "word": "ephemeral",\n      "phonetic": "/ɪˈfemərəl/",\n      "meaning": "短暂的",\n      "example": "Fame is ephemeral."\n    }\n  ]\n}\n')).toBe(true);
    expect(content.endsWith("\n")).toBe(true);
    expect(serializeWordGroup({ formatVersion: 1, words: [entry()] })).toBe(content);
    const round = parseWordGroup(content);
    expect(round.ok && round.document.words).toEqual([entry()]);
  });

  test("空组也能稳定序列化", () => {
    expect(serializeWordGroup({ formatVersion: 1, words: [] })).toBe('{\n  "formatVersion": 1,\n  "words": []\n}\n');
  });
});

describe("新建页面与完整性", () => {
  test("新建页面字段为空、id 合法且互不相同", () => {
    const first = createEntry();
    const second = createEntry();
    expect(first).toEqual({ id: first.id, word: "", phonetic: "", meaning: "", example: "" });
    expect(first.id).toMatch(/^[A-Za-z0-9_-]{1,64}$/);
    expect(first.id).not.toBe(second.id);
    expect(isEntryComplete(first)).toBe(false);
    expect(isEntryComplete(entry())).toBe(true);
    expect(isEntryComplete(entry({ meaning: "   " }))).toBe(false);
  });

  /** 兜底路径：WebView 没有 randomUUID 时仍要生成合法 id，否则后端会拒绝保存。 */
  test("随机源不可用时仍生成合法 id", () => {
    vi.stubGlobal("crypto", undefined);
    try {
      expect(createEntryId()).toMatch(/^[A-Za-z0-9_-]{1,64}$/);
    } finally {
      vi.unstubAllGlobals();
    }
  });
});

describe("音标归一化", () => {
  /** 统一 IPA 字符并补全 /…/；已有定界符保留，不重复包裹。 */
  test("统一字符并补全定界符", () => {
    expect(normalizePhonetic("i'femərəl")).toBe("/iˈfemərəl/");
    expect(normalizePhonetic("i’femərəl")).toBe("/iˈfemərəl/");
    expect(normalizePhonetic("i'femәrәl")).toBe("/iˈfemərəl/");
    expect(normalizePhonetic("i:fem")).toBe("/iːfem/");
    expect(normalizePhonetic("  /i'femərəl/  ")).toBe("/iˈfemərəl/");
    expect(normalizePhonetic("[i'fem]")).toBe("[iˈfem]");
    expect(normalizePhonetic("/i'fem")).toBe("/iˈfem/");
    expect(normalizePhonetic("")).toBe("");
    expect(normalizePhonetic("   ")).toBe("");
  });
});
