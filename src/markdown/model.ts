import { DISPLAY_MATH, INLINE_MATH, mathSource } from "./math";
import { markdownParser, type MarkdownNode } from "./parser";
import type { HeadingLevel, InlineSpan, NoteBlock, NoteHeading, NoteListItem, TableAlignment } from "./types";

/** 结构性标记：重建块文本时跳过，它们不属于正文内容。 */
const STRUCTURAL_MARKS = new Set(["QuoteMark", "HeaderMark", "ListMark", "TaskMarker", "TableDelimiter"]);

/** 纯语法标记：行内渲染时丢弃，格式由片段样式表达。 */
const DROPPED_MARKS = new Set([
  ...STRUCTURAL_MARKS, "EmphasisMark", "StrikethroughMark", "CodeMark", "LinkMark", "LinkTitle",
]);

/** 行内片段的继承状态：嵌套强调、删除线、链接都靠它传递。 */
interface SpanContext {
  kind?: InlineSpan["kind"];
  href?: string;
}

/** 从块锚点中提取卡片 ID。 */
function cardIdFromAnchor(text: string): string | null {
  const match = text.match(/\^qc-([\w-]+)/);
  return match ? match[1] : null;
}

/** 取块的原始行内内容，跳过列表符号、引用符号等结构性标记。 */
function inlineSource(node: MarkdownNode, content: string): string {
  let result = "";
  let cursor = node.from;
  for (let child = node.firstChild; child; child = child.nextSibling) {
    result += content.slice(cursor, child.from);
    if (STRUCTURAL_MARKS.has(child.name)) {
      // 跳过引用符号时连带吃掉后面的一个空格，避免块引用内的段落残留缩进。
      cursor = child.name === "QuoteMark" && content[child.to] === " " ? child.to + 1 : child.to;
      continue;
    }
    result += content.slice(child.from, child.to);
    cursor = child.to;
  }
  return (result + content.slice(cursor, node.to)).trim();
}

/** 块引用：逐行去掉引用符号，嵌套引用拍平成同一段文字。 */
function quoteText(node: MarkdownNode, content: string): string {
  return content.slice(node.from, node.to)
    .split("\n")
    .map((line) => line.replace(/^\s*(?:>\s?)+/, "").trimEnd())
    .join("\n")
    .trim();
}

/** 围栏代码块：保留语言标识与原文，未闭合围栏也能取到内容。 */
function fencedCode(node: MarkdownNode, content: string): NoteBlock {
  const info = node.getChild("CodeInfo");
  const body = node.getChild("CodeText");
  return {
    type: "code",
    language: info ? content.slice(info.from, info.to).trim() : "",
    text: body ? content.slice(body.from, body.to) : "",
  };
}

/** 缩进代码块：去掉每行开头的一层缩进。 */
function indentedCode(node: MarkdownNode, content: string): NoteBlock {
  const text = content.slice(node.from, node.to)
    .split("\n")
    .map((line) => line.replace(/^(?: {4}|\t)/, ""))
    .join("\n")
    .trimEnd();
  return { type: "code", text, language: "" };
}

/** 递归展开列表项：嵌套层级与序号写入条目，任务项记录勾选状态。 */
function collectListItems(list: MarkdownNode, content: string, depth: number, items: NoteListItem[]): void {
  const ordered = list.name === "OrderedList";
  list.getChildren("ListItem").forEach((item, position) => {
    const task = item.getChild("Task");
    const paragraph = item.getChild("Paragraph");
    const source = paragraph ?? task;
    items.push({
      text: source ? inlineSource(source, content) : "",
      depth,
      ordered,
      index: position + 1,
      task: task ? content[task.from + 1] !== " " : null,
    });
    for (const nested of [...item.getChildren("BulletList"), ...item.getChildren("OrderedList")]) {
      collectListItems(nested, content, depth + 1, items);
    }
  });
}

/** 从分隔行读取每列对齐方式，缺列按左对齐处理。 */
function columnAlignments(table: MarkdownNode, content: string, columns: number): TableAlignment[] {
  const delimiter = table.getChildren("TableDelimiter")
    .find((child) => content.slice(child.from, child.to).includes("-"));
  const cells = delimiter
    ? content.slice(delimiter.from, delimiter.to).trim().replace(/^\|/, "").replace(/\|$/, "").split("|")
    : [];
  return Array.from({ length: columns }, (_, index) => {
    const value = (cells[index] ?? "").trim();
    if (value.startsWith(":") && value.endsWith(":")) return "center" as const;
    return value.endsWith(":") ? ("right" as const) : ("left" as const);
  });
}

/** GFM 表格：表头、数据行与列对齐；单元格行内格式交给 splitInline。 */
function tableBlock(node: MarkdownNode, content: string): NoteBlock {
  const columns = node.getChild("TableHeader")?.getChildren("TableCell").length ?? 0;
  // 行尾的空单元格不会生成 TableCell 节点，统一按表头列数补空，避免列错位。
  const cells = (row: MarkdownNode | null): string[] => {
    const found = row ? row.getChildren("TableCell") : [];
    return Array.from({ length: columns }, (_, index) => (found[index] ? inlineSource(found[index], content) : ""));
  };
  return {
    type: "table",
    header: cells(node.getChild("TableHeader")),
    rows: node.getChildren("TableRow").map((row) => cells(row)),
    alignments: columnAlignments(node, content, columns),
  };
}

/** 把语法树节点转换成显示块；未知块级语法按段落原样输出，绝不吞内容。 */
function toBlock(node: MarkdownNode, content: string): NoteBlock {
  const heading = /^(?:ATX|Setext)Heading(\d)$/.exec(node.name);
  if (heading) {
    return { type: "heading", level: Number(heading[1]) as HeadingLevel, text: inlineSource(node, content) };
  }
  switch (node.name) {
    case "Paragraph": {
      const text = inlineSource(node, content);
      const cardId = cardIdFromAnchor(text);
      return cardId
        ? { type: "card", text: text.replace(/\^qc-[\w-]+\s*$/, "").trim(), cardId }
        : { type: "p", text };
    }
    case "Blockquote":
      return { type: "quote", text: quoteText(node, content) };
    case "FencedCode":
      return fencedCode(node, content);
    case "CodeBlock":
      return indentedCode(node, content);
    case "BulletList":
    case "OrderedList": {
      const items: NoteListItem[] = [];
      collectListItems(node, content, 0, items);
      return { type: "list", items };
    }
    case "Table":
      return tableBlock(node, content);
    case "HorizontalRule":
      return { type: "hr" };
    default:
      return { type: "p", text: content.slice(node.from, node.to).trim() };
  }
}

/** 把文本解析为结构化块；与编辑器共用同一个解析器，语法理解不会两边漂移。 */
export function parseNoteBlocks(content: string): NoteBlock[] {
  const blocks: NoteBlock[] = [];
  for (let node = markdownParser.parse(content).topNode.firstChild; node; node = node.nextSibling) {
    blocks.push(toBlock(node, content));
  }
  return blocks;
}

/** 无序列表按嵌套层级使用的项目符号：编辑器与显示侧共用同一份，避免两边外观分叉。 */
export const LIST_BULLETS = ["•", "◦", "▪"];

/** 列表项的项目符号：任务项用勾选框，有序列表用序号，无序列表按层级取圆点。 */
export function listMarker(item: NoteListItem): string {
  if (item.task !== null) {
    return item.task ? "☑" : "☐";
  }
  if (item.ordered) {
    return `${item.index}.`;
  }
  return LIST_BULLETS[Math.min(item.depth, LIST_BULLETS.length - 1)];
}

/** 提取大纲标题；由调用方在需要大纲时再触发，避免编辑时每次按键都全量解析。 */
export function parseNoteHeadings(content: string): NoteHeading[] {
  return parseNoteBlocks(content).flatMap((block) =>
    block.type === "heading" ? [{ text: block.text, level: block.level }] : []);
}

/** 追加文本片段：空片段丢弃，相邻同标记片段合并，减少渲染节点。 */
function pushText(spans: InlineSpan[], text: string, context: SpanContext): void {
  if (!text) return;
  const last = spans[spans.length - 1];
  if (last && last.kind === context.kind && last.href === context.href) {
    last.text += text;
    return;
  }
  spans.push({ text, ...context });
}

/** 递归展开行内节点；未支持的节点按原文输出，避免吞掉正文。 */
function renderInline(node: MarkdownNode, content: string, spans: InlineSpan[], context: SpanContext): void {
  let cursor = node.from;
  for (let child = node.firstChild; child; child = child.nextSibling) {
    pushText(spans, content.slice(cursor, child.from), context);
    renderNode(child, content, spans, context);
    cursor = child.to;
  }
  pushText(spans, content.slice(cursor, node.to), context);
}

/** 链接目标：取 URL 节点，没有目标的链接按普通文字处理。 */
function linkTarget(node: MarkdownNode, content: string): string | null {
  const url = node.getChild("URL");
  return url ? content.slice(url.from, url.to) : null;
}

/** 链接文字范围：位于前两个 LinkMark 之间，标记节点本身不进入片段。 */
function linkLabel(node: MarkdownNode): { from: number; to: number } {
  const marks = node.getChildren("LinkMark");
  const [open, close] = marks;
  return open && close && close.from > open.from ? { from: open.to, to: close.from } : { from: node.from, to: node.to };
}

/** 渲染一段独立 Markdown 文本；块之间用换行连接。 */
function renderFragment(text: string, spans: InlineSpan[], context: SpanContext): void {
  const document = markdownParser.parse(text).topNode;
  let first = true;
  for (let node = document.firstChild; node; node = node.nextSibling) {
    if (!first) pushText(spans, "\n", context);
    first = false;
    renderInline(node, text, spans, context);
  }
}

/** 按节点类型展开行内内容；未知节点按原文输出，保证不吞字。 */
function renderNode(node: MarkdownNode, content: string, spans: InlineSpan[], context: SpanContext): void {
  if (DROPPED_MARKS.has(node.name)) return;
  switch (node.name) {
    case "HardBreak":
      pushText(spans, "\n", context);
      return;
    case INLINE_MATH:
    case DISPLAY_MATH:
      // 公式整段作为一个片段，源码原样交给渲染端，不参与相邻片段合并。
      spans.push({ text: mathSource(node, content), kind: "math", display: node.name === DISPLAY_MATH });
      return;
    case "InlineCode": {
      const code = content.slice(node.from, node.to).replace(/^`+/, "").replace(/`+$/, "");
      pushText(spans, code, { ...context, kind: "code" });
      return;
    }
    case "StrongEmphasis":
      renderInline(node, content, spans, { ...context, kind: "bold" });
      return;
    case "Emphasis":
      renderInline(node, content, spans, { ...context, kind: "italic" });
      return;
    case "Strikethrough":
      renderInline(node, content, spans, { ...context, kind: "strike" });
      return;
    case "Link":
    case "Autolink": {
      const href = linkTarget(node, content);
      const label = node.name === "Autolink" ? { from: node.from, to: node.to } : linkLabel(node);
      const labelText = href && node.name === "Autolink"
        ? href
        : content.slice(label.from, label.to);
      renderFragment(labelText, spans, href ? { ...context, kind: "link", href } : context);
      return;
    }
    default:
      pushText(spans, content.slice(node.from, node.to), context);
      return;
  }
}

/** 把一行文本切成普通文字与行内标记片段；块级结构由 parseNoteBlocks 负责。 */
export function splitInline(text: string): InlineSpan[] {
  const spans: InlineSpan[] = [];
  renderFragment(text, spans, {});
  return spans;
}

/** 估算笔记正文字数：按非空白字符计数。 */
export function countContentWords(content: string): number {
  return content.replace(/\s+/g, "").length;
}
