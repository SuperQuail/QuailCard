import type { MarkdownParser } from "@lezer/markdown";
import { expect, test } from "vitest";
import { buildOutline } from "./outline";

/** ATX 与 Setext 标题都要进入大纲，from 必须指向标题行行首，跳转才不会落到上一段。 */
test("收集 ATX 与 Setext 标题并记录行首偏移", () => {
  const content = "# 一级\n\n正文\n\n二级\n---\n\n### 三级";
  expect(buildOutline(content)).toEqual([
    { level: 1, text: "一级", from: 0 },
    { level: 2, text: "二级", from: content.indexOf("二级") },
    { level: 3, text: "三级", from: content.indexOf("### 三级") },
  ]);
});

/** 围栏代码块与缩进代码块里的 # 是代码而不是标题，不能被大纲误收。 */
test("代码块里的 # 不算标题", () => {
  const content = "# 标题\n\n```md\n# 不是标题\n```\n\n    # 也不是标题";
  expect(buildOutline(content)).toEqual([{ level: 1, text: "标题", from: 0 }]);
});

/** 标题里的行内格式符号要剥离，只留可读文字；中英文与链接文字都保留。 */
test("行内 Markdown 符号被剥离", () => {
  const content = "## **粗** text\n\n### *斜* `code` ~~删~~ [链接](https://example.com)";
  expect(buildOutline(content).map((item) => item.text)).toEqual(["粗 text", "斜 code 删 链接"]);
});

/** 空文档没有标题，调用方据此显示空状态。 */
test("空文档返回空大纲", () => {
  expect(buildOutline("")).toEqual([]);
  expect(buildOutline("   \n\t ")).toEqual([]);
});

/** frontmatter 扩展的 Frontmatter 节点整棵子树都要跳过，避免 YAML 行被当成标题。 */
test("Frontmatter 节点及其子树被跳过", () => {
  const content = "---\ntitle: 文档\n---\n# 正文标题";
  const headingFrom = content.indexOf("# 正文标题");
  // 假树模拟 frontmatter 扩展：即使 Frontmatter 子树里出现标题节点，也不能进入大纲。
  const nested = { name: "ATXHeading2", from: 4, to: 16, firstChild: null, nextSibling: null };
  const heading = { name: "ATXHeading1", from: headingFrom, to: content.length, firstChild: null, nextSibling: null };
  const frontmatter = { name: "Frontmatter", from: 0, to: headingFrom, firstChild: nested, nextSibling: heading };
  const parser = {
    parse: () => ({ topNode: { name: "Document", from: 0, to: content.length, firstChild: frontmatter, nextSibling: null } }),
  } as unknown as MarkdownParser;
  expect(buildOutline(content, parser)).toEqual([{ level: 1, text: "正文标题", from: headingFrom }]);
});
