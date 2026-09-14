import { tags as t } from "@lezer/highlight";
import type { Element, InlineContext, MarkdownConfig } from "@lezer/markdown";

/** 行内公式 `$…$` 的节点名。 */
export const INLINE_MATH = "InlineMath";
/** 单行块级公式 `$$…$$` 的节点名。 */
export const DISPLAY_MATH = "DisplayMath";
/** 定界符节点名。 */
export const MATH_MARK = "MathMark";

/** 空白判定：公式内容不允许以空白开头或结尾，也不允许跨行。 */
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

/**
 * 只解析单行的 `$…$` 与 `$$…$$`。
 * 采用 Pandoc 行内公式规则：开定界符后不能是空白，闭定界符前不能是空白、后面不能紧跟数字，
 * 内容不得跨行；任一条不满足就返回 -1，整段按原文保留——`价格 $5 到 $10` 因此不会被当成公式，
 * 真需要字面量时可以写 \$5 或用反引号包起来。
 */
function parseMath(cx: InlineContext, next: number, pos: number): number {
  if (next !== 36 /* $ */) return -1;
  const display = cx.char(pos + 1) === 36;
  const width = display ? 2 : 1;
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

/** 单行公式扩展：多行 `$$` 一律按原文保留，不做任何改写。 */
export const mathExtension: MarkdownConfig = {
  defineNodes: [
    { name: MATH_MARK, style: t.processingInstruction },
    { name: INLINE_MATH, style: t.special(t.content) },
    { name: DISPLAY_MATH, style: t.special(t.content) },
  ],
  parseInline: [{ name: "Math", parse: parseMath }],
};
