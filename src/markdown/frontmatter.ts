import { tags as t } from "@lezer/highlight";
import type { BlockContext, BlockParser, Line, MarkdownConfig } from "@lezer/markdown";

/** frontmatter 块的节点名：显示侧、装饰与测试都只认这一个名字。 */
export const FRONTMATTER = "Frontmatter";

/** 允许的 frontmatter 总行数（含首尾定界行）；超过后按普通 Markdown 解析。 */
export const FRONTMATTER_MAX_LINES = 200;

/**
 * BlockContext 的 input 是解析器输入文本（CodeMirror 的 Text 或字符串输入）。
 * 它不在公开类型里，但运行时一定存在；判定闭合必须先于消耗行，而公开 API 没有回退能力，
 * 只能借助它做只读前瞻。这里只依赖所有 Input 都实现的 length/read 两个稳定成员。
 */
interface FrontmatterInput {
  readonly length: number;
  read?: (from: number, to: number) => string;
  sliceString?: (from: number, to: number) => string;
  chunk?: (from: number) => string;
}

/** 定界行：允许行尾空白（YAML 编辑器常自动补空格），但内容必须仍是 ---。 */
function isDelimiter(line: string): boolean {
  return line.trimEnd() === "---";
}

/** 前瞻文本上限：200 行正常 frontmatter 远小于此值，避免为坏数据复制整篇长笔记。 */
const MAX_LOOKAHEAD_CHARS = 128 * 1024;

/**
 * 取出文档开头最多 maxLines 行文本，失败返回 null。
 *
 * 关键的契约细节：编辑器里的解析输入是 CodeMirror 的 DocInput，它的 chunk(0)
 * 只给一个分块而不是全文，因此必须用 read(0, n) 按需读取；读到上限仍没结束时，
 * 最后一行可能被截断，不能用于闭合判定（避免把 ----- 的前缀误当 ---）。
 */
function readOpeningLines(cx: BlockContext, maxLines: number): string[] | null {
  const input = (cx as unknown as { input?: FrontmatterInput }).input;
  if (!input) return null;
  /** 按需读取输入前缀：DocInput 只保证 read 可用，chunk 只给一个分块。 */
  const read = (to: number): string => {
    if (input.read) return input.read(0, to);
    if (input.sliceString) return input.sliceString(0, to);
    return input.chunk?.(0) ?? "";
  };
  let limit = Math.min(input.length, MAX_LOOKAHEAD_CHARS);
  let text = read(limit);
  for (;;) {
    const complete = limit >= input.length;
    const lines = text.split("\n");
    // 只读到半行时丢弃最后一行，保证用于闭合判定的每一行都是完整行。
    const visible = (complete ? lines : lines.slice(0, -1)).slice(0, maxLines);
    // 行数够判定、到文末或到前瞻上限就停止，避免为长行笔记无限复制正文。
    if (visible.length >= maxLines || complete || limit >= MAX_LOOKAHEAD_CHARS) return visible;
    limit = Math.min(input.length, Math.min(limit * 2, MAX_LOOKAHEAD_CHARS));
    text = read(limit);
  }
}

/**
 * frontmatter 块解析器。契约：
 * - 只认文档第 0 行恰好是 --- 的情况；任何前置空行或缩进都会让它退回普通语法。
 * - 闭合行同样必须是 ---；在 FRONTMATTER_MAX_LINES 行内找不到闭合就完全不生效，
 *   把开头这行留给 HorizontalRule，正文不会被吞掉。
 * - 节点范围包含首尾两条定界行，正文不进入 Frontmatter 节点。
 */
const frontmatterBlockParser: BlockParser = {
  name: "Frontmatter",
  // --- 单行会被默认解析成分隔线，必须抢在它之前判定。
  before: "HorizontalRule",
  parse(cx: BlockContext, line: Line): boolean {
    if (cx.lineStart !== 0 || !isDelimiter(line.text)) return false;
    const lines = readOpeningLines(cx, FRONTMATTER_MAX_LINES);
    if (!lines) return false;
    let closeIndex = -1;
    for (let index = 1; index < lines.length; index++) {
      if (isDelimiter(lines[index]!)) {
        closeIndex = index;
        break;
      }
    }
    if (closeIndex < 0) return false;
    // 前瞻确认后再消耗：先移动到闭合行，节点覆盖到行尾，再越过闭合行交给块解析循环。
    for (let index = 0; index < closeIndex; index++) cx.nextLine();
    const to = cx.lineStart + line.text.length;
    cx.addElement(cx.elt(FRONTMATTER, 0, to));
    cx.nextLine();
    return true;
  },
};

/** Markdown 语法扩展：登记 Frontmatter 节点与块解析器，编辑器与显示侧共用。 */
export const frontmatterMarkdown: MarkdownConfig = {
  defineNodes: [{ name: FRONTMATTER, block: true, style: t.meta }],
  parseBlock: [frontmatterBlockParser],
};
