import { describe, expect, test } from "vitest";
import { FRONTMATTER, FRONTMATTER_MAX_LINES } from "./frontmatter";
import { markdownParser } from "./parser";

/** 取语法树第一个块节点名，便于断言 frontmatter 是否被识别。 */
function firstBlockName(doc: string): string | undefined {
  return markdownParser.parse(doc).topNode.firstChild?.name;
}

/** 语法树里是否出现 Frontmatter 节点。 */
function hasFrontmatter(doc: string): boolean {
  let found = false;
  markdownParser.parse(doc).iterate({
    enter(node) {
      if (node.name === FRONTMATTER) found = true;
    },
  });
  return found;
}

describe("frontmatter 块语法", () => {
  test("第 0 行是 --- 且闭合时产出 Frontmatter，范围含首尾定界行", () => {
    const doc = "---\ntags: [a, b]\naliases:\n  - 甲\n---\n正文";
    const first = markdownParser.parse(doc).topNode.firstChild;
    expect(first?.name).toBe(FRONTMATTER);
    expect(doc.slice(first!.from, first!.to)).toBe("---\ntags: [a, b]\naliases:\n  - 甲\n---");
    expect(first!.nextSibling?.name).toBe("Paragraph");
  });

  test("前置空行或缩进都不会误判成 frontmatter", () => {
    expect(firstBlockName("\n---\na: 1\n---\n正文")).not.toBe(FRONTMATTER);
    expect(firstBlockName(" ---\na: 1\n---\n正文")).not.toBe(FRONTMATTER);
  });

  test("单行 --- 仍是分隔线", () => {
    expect(firstBlockName("---")).toBe("HorizontalRule");
  });

  test("找不到闭合行时不生效：--- 保持分隔线，后文正常解析", () => {
    const doc = "---\n# 标题\n正文";
    const tree = markdownParser.parse(doc);
    expect(tree.topNode.firstChild?.name).toBe("HorizontalRule");
    expect(tree.topNode.getChildren("ATXHeading1")).toHaveLength(1);
    expect(hasFrontmatter(doc)).toBe(false);
  });

  test("闭合出现在第 200 行（含）以内仍然生效", () => {
    const lines = ["---"];
    for (let index = 1; index < FRONTMATTER_MAX_LINES - 1; index++) lines.push(`k${index}: v`);
    lines.push("---", "正文");
    const doc = lines.join("\n");
    const first = markdownParser.parse(doc).topNode.firstChild;
    expect(first?.name).toBe(FRONTMATTER);
    expect(doc.slice(first!.from, first!.to).split("\n").length).toBe(FRONTMATTER_MAX_LINES);
  });

  test("闭合行超出 200 行就不生效，正文不会被吞掉", () => {
    const lines = ["---"];
    for (let index = 1; index < FRONTMATTER_MAX_LINES; index++) lines.push(`k${index}: v`);
    lines.push("---", "# 标题", "正文");
    const doc = lines.join("\n");
    expect(hasFrontmatter(doc)).toBe(false);
    expect(markdownParser.parse(doc).topNode.firstChild?.name).toBe("HorizontalRule");
    expect(markdownParser.parse(doc).topNode.getChildren("ATXHeading1")).toHaveLength(1);
  });

  test("定界行允许行尾空白", () => {
    const doc = "---  \na: 1\n---\t\n正文";
    expect(firstBlockName(doc)).toBe(FRONTMATTER);
  });
});
