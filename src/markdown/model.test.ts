import { describe, expect, test } from "vitest";
import { countContentWords, listMarker, parseNoteBlocks, splitInline } from "./model";

describe("块级投影", () => {
  test("一到六级标题与 Setext 标题都识别，标记不进入正文", () => {
    const content = "# 一级\n\n###### 六级\n\nSetext 一级\n===\n\nSetext 二级\n---";
    expect(parseNoteBlocks(content)).toEqual([
      { type: "heading", level: 1, text: "一级" },
      { type: "heading", level: 6, text: "六级" },
      { type: "heading", level: 1, text: "Setext 一级" },
      { type: "heading", level: 2, text: "Setext 二级" },
    ]);
  });

  test("引用逐行去符号，嵌套引用拍平", () => {
    expect(parseNoteBlocks("> 外层\n> > 内层")).toEqual([{ type: "quote", text: "外层\n内层" }]);
  });

  test("围栏代码保留语言，缩进代码去掉一层缩进", () => {
    expect(parseNoteBlocks("```ts\nconst a = 1;\n```")).toEqual([{ type: "code", language: "ts", text: "const a = 1;" }]);
    expect(parseNoteBlocks("    缩进代码")).toEqual([{ type: "code", language: "", text: "缩进代码" }]);
  });

  test("列表带出层级、序号与任务状态，空行分隔的列表各自成块", () => {
    const content = "- 外\n  - 内\n- [ ] 待办\n- [x] 完成\n\n1. 第一\n2. 第二";
    expect(parseNoteBlocks(content)).toEqual([
      {
        type: "list",
        items: [
          { text: "外", depth: 0, ordered: false, index: 1, task: null },
          { text: "内", depth: 1, ordered: false, index: 1, task: null },
          { text: "待办", depth: 0, ordered: false, index: 2, task: false },
          { text: "完成", depth: 0, ordered: false, index: 3, task: true },
        ],
      },
      {
        type: "list",
        items: [
          { text: "第一", depth: 0, ordered: true, index: 1, task: null },
          { text: "第二", depth: 0, ordered: true, index: 2, task: null },
        ],
      },
    ]);
  });

  test("表格带出对齐方式，单元格保留行内标记原文", () => {
    const content = "| A | B |\n| :--- | ---: |\n| **x** | `y` |\n| a\\|b | |";
    expect(parseNoteBlocks(content)).toEqual([{
      type: "table",
      header: ["A", "B"],
      rows: [["**x**", "`y`"], ["a\\|b", ""]],
      alignments: ["left", "right"],
    }]);
  });

  test("分隔线与卡片锚点保留原有语义", () => {
    expect(parseNoteBlocks("---")).toEqual([{ type: "hr" }]);
    expect(parseNoteBlocks("重点内容 ^qc-abc")).toEqual([{ type: "card", text: "重点内容", cardId: "abc" }]);
  });

  test("HTML 一律作为文本，不产生任何元素", () => {
    expect(parseNoteBlocks("<div>内容</div>")).toEqual([{ type: "p", text: "<div>内容</div>" }]);
  });
});

describe("行内投影", () => {
  test("粗体、斜体、行内代码、删除线与链接各自成段", () => {
    expect(splitInline("**粗** *斜* `代` ~~删~~ [文字](https://a.com)")).toEqual([
      { text: "粗", kind: "bold" },
      { text: " " },
      { text: "斜", kind: "italic" },
      { text: " " },
      { text: "代", kind: "code" },
      { text: " " },
      { text: "删", kind: "strike" },
      { text: " " },
      { text: "文字", kind: "link", href: "https://a.com" },
    ]);
  });

  test("嵌套强调正确展开，反引号与链接目标不进入正文", () => {
    expect(splitInline("a **b *c* d** e").map((span) => ({ ...span }))).toEqual([
      { text: "a " },
      { text: "b ", kind: "bold" },
      { text: "c", kind: "italic" },
      { text: " d", kind: "bold" },
      { text: " e" },
    ]);
    expect(splitInline("[笔记](b.md)").map((span) => span.text).join("")).toBe("笔记");
  });

  test("转义反斜杠逐字保留，硬换行成为换行", () => {
    expect(splitInline("\\*不是斜体\\*")).toEqual([{ text: "\\*不是斜体\\*" }]);
    expect(splitInline("一  \n二")).toEqual([{ text: "一\n二" }]);
  });

  test("多行 LaTeX 逐字保留，单行公式只把定界符当语法", () => {
    // 多行 $$ 不识别为公式，整段按原文保留。
    for (const source of ["$$\n\\int_0^1 x^2\\,dx\n$$", "$$\nE=mc^2\n$$"]) {
      expect(parseNoteBlocks(source)).toEqual([{ type: "p", text: source }]);
      expect(splitInline(source).map((span) => span.text).join("")).toBe(source);
    }
    // 单行公式：块文本保留原始 Markdown，行内投影只剥掉定界符，公式体逐字不动。
    const formulas: Array<[string, string]> = [
      ["$$\\int_0^1 x^2\\,dx$$", "\\int_0^1 x^2\\,dx"],
      ["$\\frac{1}{2}$", "\\frac{1}{2}"],
      ["$\\alpha_1$", "\\alpha_1"],
      ["$$E=mc^2$$", "E=mc^2"],
      ["$\\{x\\} \\_y \\% z$", "\\{x\\} \\_y \\% z"],
    ];
    for (const [source, body] of formulas) {
      expect(parseNoteBlocks(source)).toEqual([{ type: "p", text: source }]);
      expect(splitInline(source)).toEqual([{ text: body, kind: "math", display: source.startsWith("$$") }]);
    }
  });

  test("图片、裸链接与实体按原文显示，不静默改写内容", () => {
    expect(splitInline("![图](a.png)")).toEqual([{ text: "![图](a.png)" }]);
    expect(splitInline("见 www.example.com")).toEqual([{ text: "见 www.example.com" }]);
    expect(splitInline("&amp;")).toEqual([{ text: "&amp;" }]);
  });

  test("未启用的扩展语法保持原样，日常文本不被误判", () => {
    expect(splitInline("时间 12:30:45，比例 3:1:2")).toEqual([{ text: "时间 12:30:45，比例 3:1:2" }]);
    expect(splitInline("H~2~O 与 ^_^ 与 :smile:")).toEqual([{ text: "H~2~O 与 ^_^ 与 :smile:" }]);
    expect(splitInline("snake_case_name 与 2 * 3 = 6")).toEqual([{ text: "snake_case_name 与 2 * 3 = 6" }]);
    expect(splitInline("**未闭合的粗体")).toEqual([{ text: "**未闭合的粗体" }]);
  });

test("单行公式识别为数学片段，多行与普通美元符号保持原文", () => {
    expect(splitInline("$x^2$")).toEqual([{ text: "x^2", kind: "math", display: false }]);
    expect(splitInline("$$E=mc^2$$")).toEqual([{ text: "E=mc^2", kind: "math", display: true }]);
    expect(splitInline("公式 $\\frac{1}{2}$ 结束")).toEqual([
      { text: "公式 " },
      { text: "\\frac{1}{2}", kind: "math", display: false },
      { text: " 结束" },
    ]);
  });

  test("美元符号的常见误伤不会被当成公式", () => {
    for (const source of [
      "价格 $5 到 $10",
      "US$100 和 $200",
      "$ x$ 前导空格",
      "$x $ 结尾空格",
      "多行 $$\nE=mc^2\n$$ 不识别",
      "`$5` 用反引号写字面量",
      "a$b 未闭合",
    ]) {
      expect(splitInline(source).some((span) => span.kind === "math")).toBe(false);
    }
  });

  test("公式所在的段落文本原样保留，交给行内投影再解析", () => {
    const blocks = parseNoteBlocks("行内 $a^2+b^2=c^2$ 与 $$\\int_0^1 x\\,dx$$");
    expect(blocks).toEqual([{ type: "p", text: "行内 $a^2+b^2=c^2$ 与 $$\\int_0^1 x\\,dx$$" }]);
  });

  test("块文本交给行内投影时不吞字", () => {
    const blocks = parseNoteBlocks("**音标**: /spiːk/，*动词* 还有 `code` 与结尾");
    expect(blocks).toEqual([{ type: "p", text: "**音标**: /spiːk/，*动词* 还有 `code` 与结尾" }]);
    const text = (blocks[0] as { text: string }).text;
    expect(splitInline(text).map((span) => span.text).join("")).toBe("音标: /spiːk/，动词 还有 code 与结尾");
  });
});

describe("列表符号", () => {
  test("编辑器与显示侧共用同一份符号规则", () => {
    const item = { text: "项目", index: 1, ordered: false, task: null };
    expect(listMarker({ ...item, depth: 0 })).toBe("•");
    expect(listMarker({ ...item, depth: 1 })).toBe("◦");
    expect(listMarker({ ...item, depth: 5 })).toBe("▪");
    expect(listMarker({ ...item, depth: 0, ordered: true, index: 4 })).toBe("4.");
    expect(listMarker({ ...item, depth: 0, task: false })).toBe("☐");
    expect(listMarker({ ...item, depth: 0, task: true })).toBe("☑");
  });
});

describe("字数统计", () => {
  test("按非空白字符计数", () => {
    expect(countContentWords("a b\nc\td")).toBe(4);
  });
});
