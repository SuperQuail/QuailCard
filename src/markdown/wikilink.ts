import { tags as t } from "@lezer/highlight";
import type { Element, InlineContext, InlineParser, MarkdownConfig } from "@lezer/markdown";

/** 双链节点名：装饰层与测试只认这一个名字。 */
export const WIKILINK = "WikiLink";
/** 目标子节点（不含 |显示 部分与两侧方括号，已去掉首尾空白）。 */
export const WIKILINK_TARGET = "WikiLinkTarget";
/** 显示名子节点；没有 | 或 | 后为空时不产生。 */
export const WIKILINK_LABEL = "WikiLinkLabel";

const BANG = 33;
const OPEN = 91;
const CLOSE = 93;
const PIPE = 124;
const NEWLINE = 10;
const CARRIAGE = 13;

/** 空白判定：目标与显示名两侧的空白不属于内容。 */
function isSpace(code: number): boolean {
  return code === 32 || code === 9 || code === 10 || code === 13;
}

/** 去掉首尾空白后的位置区间；区间为空返回 null。 */
function trimRange(cx: InlineContext, from: number, to: number): { from: number; to: number } | null {
  let start = from;
  let end = to;
  while (start < end && isSpace(cx.char(start))) start++;
  while (end > start && isSpace(cx.char(end - 1))) end--;
  return start < end ? { from: start, to: end } : null;
}

/**
 * 解析 [[目标]] 系列语法。支持四种形态与内嵌图形式：
 * [[目标]]、[[目标|显示]]、[[目标#标题]]、[[目标#标题|显示]]、![[目标]]。
 * 契约：必须在同一行闭合，且目标不能为空，否则返回 -1 交给默认 Link 解析器，
 * 这样未闭合的 [[ 仍按普通文字显示，不会吞掉后文。
 */
function parseWikiLink(cx: InlineContext, next: number, pos: number): number {
  const embed = next === BANG;
  if (embed) {
    if (cx.char(pos + 1) !== OPEN || cx.char(pos + 2) !== OPEN) return -1;
  } else if (next !== OPEN || cx.char(pos + 1) !== OPEN) {
    return -1;
  }
  const contentFrom = embed ? pos + 3 : pos + 2;
  let close = -1;
  for (let index = contentFrom; index < cx.end; index++) {
    const code = cx.char(index);
    if (code === NEWLINE || code === CARRIAGE) return -1;
    if (code === CLOSE && cx.char(index + 1) === CLOSE) {
      close = index;
      break;
    }
  }
  if (close < 0) return -1;
  const pipe = findPipe(cx, contentFrom, close);
  const targetRange = trimRange(cx, contentFrom, pipe < 0 ? close : pipe);
  if (!targetRange) return -1;
  const children: Element[] = [cx.elt(WIKILINK_TARGET, targetRange.from, targetRange.to)];
  if (pipe >= 0) {
    const labelRange = trimRange(cx, pipe + 1, close);
    if (labelRange) children.push(cx.elt(WIKILINK_LABEL, labelRange.from, labelRange.to));
  }
  return cx.addElement(cx.elt(WIKILINK, pos, close + 2, children));
}

/** 找 | 分隔符位置；目标内部的 | 不允许出现，因此取第一个。 */
function findPipe(cx: InlineContext, from: number, to: number): number {
  for (let index = from; index < to; index++) {
    if (cx.char(index) === PIPE) return index;
  }
  return -1;
}

/** 语法扩展：抢在默认 Link 解析器之前识别 [[，避免它先把第一个 [ 当链接起点。 */
const wikiLinkParser: InlineParser = {
  name: "WikiLink",
  before: "Link",
  parse: parseWikiLink,
};

/** Markdown 语法扩展：登记 WikiLink 三个节点与行内解析器。 */
export const wikilinkMarkdown: MarkdownConfig = {
  defineNodes: [
    { name: WIKILINK, style: t.link },
    { name: WIKILINK_TARGET, style: t.url },
    { name: WIKILINK_LABEL, style: t.string },
  ],
  parseInline: [wikiLinkParser],
};

/** 结构上等同于 lezer 的 SyntaxNode，装饰层不必把编辑器类型带进语法模块。 */
export interface WikiLinkNode {
  from: number;
  to: number;
  getChild(name: string): { from: number; to: number } | null;
}

/** 一段双链的解析结果：位置、目标原文、可选的显示名与是否为内嵌。 */
export interface WikiLinkSpan {
  from: number;
  to: number;
  /** | 之前的原文（已去首尾空白），可能带 #标题。 */
  target: string;
  /** | 之后的显示名；没有时 label 为 null。 */
  label: string | null;
  /** 是否为 ![[...]] 内嵌形式。 */
  embed: boolean;
}

/**
 * 读取双链节点的目标与显示名。
 * 契约：只依赖目标/显示名两个子节点的位置，编辑器与显示侧共用，不重复写字符串切分。
 */
export function wikiLinkSpan(node: WikiLinkNode, content: string): WikiLinkSpan {
  const targetNode = node.getChild(WIKILINK_TARGET);
  const labelNode = node.getChild(WIKILINK_LABEL);
  const raw = content.slice(node.from, node.to);
  return {
    from: node.from,
    to: node.to,
    target: targetNode ? content.slice(targetNode.from, targetNode.to) : "",
    label: labelNode ? content.slice(labelNode.from, labelNode.to) : null,
    embed: raw.startsWith("![["),
  };
}

/** 阅读态展示文字：优先显示名，否则取目标里 #标题 之前的笔记部分。 */
export function wikiLinkDisplayText(span: WikiLinkSpan): string {
  if (span.label) return span.label;
  const { note } = splitWikiTarget(span.target);
  return note || span.target;
}

/**
 * 把 [[目标]] 的目标拆成"笔记部分 + #标题部分"。
 * 契约：# 之后是笔记内标题锚点，不参与笔记路径解析；App 打开链接时据此定位行。
 */
export function splitWikiTarget(target: string): { note: string; heading: string | null } {
  const index = target.indexOf("#");
  if (index < 0) return { note: target.trim(), heading: null };
  const heading = target.slice(index + 1).trim();
  return { note: target.slice(0, index).trim(), heading: heading || null };
}
