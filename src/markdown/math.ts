import { tags as t } from "@lezer/highlight";
import type { Element, InlineContext, MarkdownConfig } from "@lezer/markdown";

/** 行内公式 `$…$` 的节点名。 */
export const INLINE_MATH = "InlineMath";
/** 块级公式 `$$…$$` 的节点名：既有写在一行里的写法，也有独占多行的 `$$` 围栏。 */
export const DISPLAY_MATH = "DisplayMath";
/** 定界符节点名。 */
export const MATH_MARK = "MathMark";

/** 空白判定：行内公式的内容不允许以空白开头或结尾，也不允许跨行。 */
function isSpace(code: number): boolean {
  return code === 32 || code === 9 || code === 10 || code === 13;
}

/** 数字判定：闭定界符后紧跟数字时不闭合。 */
function isDigit(code: number): boolean {
  return code >= 48 && code <= 57;
}

/** 取出公式源码：去掉定界符后交给 KaTeX。 */
export function mathSource(node: { name: string; from: number; to: number }, content: string): string {
  const width = node.name === DISPLAY_MATH ? 2 : 1;
  return content.slice(node.from + width, node.to - width);
}

/** 行首位置：向前找换行，不越过当前行内片段。 */
function lineStart(cx: InlineContext, pos: number): number {
  let index = pos;
  while (index > cx.offset && cx.char(index - 1) !== 10 && cx.char(index - 1) !== 13) index--;
  return index;
}

/** 行尾位置：向后找换行，不越过当前行内片段。 */
function lineEnd(cx: InlineContext, pos: number): number {
  let index = pos;
  while (index < cx.end && cx.char(index) !== 10 && cx.char(index) !== 13) index++;
  return index;
}

/** 区间内是否只有空格与制表符。 */
function blank(cx: InlineContext, from: number, to: number): boolean {
  for (let index = from; index < to; index++) {
    const code = cx.char(index);
    if (code !== 32 && code !== 9) return false;
  }
  return true;
}

/**
 * 多行块级公式：`$$` 独占一行开始，直到另一个只有 `$$` 的行结束。
 * 返回闭合定界符之后的位置；写法不满足（开定界符不独占一行、没有闭合行、
 * 中间出现空行）就返回 -1，让这段文本按原文保留，绝不吞掉正文。
 */
function parseBlockMath(cx: InlineContext, pos: number): number {
  if (!blank(cx, lineStart(cx, pos), pos)) return -1;
  const openEnd = lineEnd(cx, pos);
  if (!blank(cx, pos + 2, openEnd)) return -1;
  for (let start = openEnd + 1; start < cx.end;) {
    const end = lineEnd(cx, start);
    const text = cx.slice(start, end);
    const trimmed = text.trim();
    // 空行意味着段落已经结束，闭合定界符不可能再出现。
    if (!trimmed) return -1;
    if (trimmed === "$$") {
      const lead = text.length - text.trimStart().length;
      return start + lead + 2;
    }
    start = end + 1;
  }
  return -1;
}

/**
 * `$…$` 只允许单行；`$$…$$` 既可写在一行里，也可用独占一行的 `$$` 围起来写成多行。
 * 行内写法沿用 Pandoc 规则：开定界符后不能是空白，闭定界符前不能是空白、后面不能紧跟数字，
 * 内容不得跨行；任一条不满足就返回 -1，整段按原文保留——`价格 $5 到 $10` 因此不会被当成公式，
 * 真需要字面量时可以写 \$5 或用反引号包起来。
 */
function parseMath(cx: InlineContext, next: number, pos: number): number {
  if (next !== 36 /* $ */) return -1;
  const display = cx.char(pos + 1) === 36;
  const width = display ? 2 : 1;
  // 多行围栏写法优先：它独占整行，匹配成功就整段交给 KaTeX。
  if (display) {
    const blockEnd = parseBlockMath(cx, pos);
    if (blockEnd > 0) {
      const children: Element[] = [cx.elt(MATH_MARK, pos, pos + 2), cx.elt(MATH_MARK, blockEnd - 2, blockEnd)];
      return cx.addElement(cx.elt(DISPLAY_MATH, pos, blockEnd, children));
    }
  }
  const contentFrom = pos + width;
  if (contentFrom >= cx.end || isSpace(cx.char(contentFrom))) return -1;
  const children: Element[] = [cx.elt(MATH_MARK, pos, contentFrom)];
  for (let i = contentFrom; i < cx.end; i++) {
    const code = cx.char(i);
    if (code === 10 || code === 13) return -1;
    if (code === 92 /* \ */ && i + 1 < cx.end) {
      children.push(cx.elt("Escape", i, i + 2));
      i++;
      continue;
    }
    if (code !== 36) continue;
    if (display && cx.char(i + 1) !== 36) continue;
    if (i === contentFrom || isSpace(cx.char(i - 1))) continue;
    if (!display && isDigit(cx.char(i + 1))) continue;
    const end = i + width;
    children.push(cx.elt(MATH_MARK, i, end));
    return cx.addElement(cx.elt(display ? DISPLAY_MATH : INLINE_MATH, pos, end, children));
  }
  return -1;
}

/** 公式扩展：单行 `$…$`、单行 `$$…$$` 与多行 `$$` 围栏共用同一套节点类型。 */
export const mathExtension: MarkdownConfig = {
  defineNodes: [
    { name: MATH_MARK, style: t.processingInstruction },
    { name: INLINE_MATH, style: t.special(t.content) },
    { name: DISPLAY_MATH, style: t.special(t.content) },
  ],
  parseInline: [{ name: "Math", parse: parseMath }],
};
