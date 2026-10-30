import type { MarkdownParser } from "@lezer/markdown";
import { splitInline } from "../markdown/model";
import { markdownParser, type MarkdownNode } from "../markdown/parser";

/**
 * 大纲条目：level 为 1-6，from 为标题行的起始偏移，供编辑器定位与跳转。
 * 该类型是右栏与编辑器之间的跳转契约，调用方不要改名或改字段语义。
 */
export interface OutlineItem {
  /** 标题层级 1-6。 */
  level: number;
  /** 去掉 # 与行内标记后的纯文本。 */
  text: string;
  /** 标题在正文中的起始偏移（编辑器文档坐标）。 */
  from: number;
}

/** 标题节点名：ATX 与 Setext 共用同一套层级提取规则。 */
const HEADING_NAME = /^(?:ATX|Setext)Heading([1-6])$/;

/** frontmatter 节点名：由 frontmatter 语法扩展提供；出现时整棵子树都不属于笔记正文。 */
const FRONTMATTER_NODE = "Frontmatter";

/**
 * 提取标题纯文本：复用显示侧的 splitInline 剥离 # 标记、强调、代码、删除线与链接外壳，
 * 保证大纲与正文渲染对“行内 Markdown 符号”的理解永远一致，而不是各写一套正则。
 */
function headingText(node: MarkdownNode, content: string): string {
  return splitInline(content.slice(node.from, node.to)).map((span) => span.text).join("").trim();
}

/**
 * 递归收集标题。跳过 frontmatter 子树（其中可能出现被误判成标题的 YAML 行），
 * 引用块、列表项里的标题同样收进大纲；跳转位置用标题节点自身的 from，也就是该行行首。
 */
function collectHeadings(node: MarkdownNode, content: string, items: OutlineItem[]): void {
  for (let child = node.firstChild; child; child = child.nextSibling) {
    if (child.name === FRONTMATTER_NODE) continue;
    const name = HEADING_NAME.exec(child.name);
    if (name) {
      items.push({ level: Number(name[1]), text: headingText(child, content), from: child.from });
      continue;
    }
    collectHeadings(child, content, items);
  }
}

/**
 * 解析 Markdown 正文生成大纲；空文档直接返回空数组，调用方据此显示“没有标题”。
 * parser 默认是与显示侧共享的 markdownParser；可选参数只用于测试注入带 Frontmatter 节点的
 * 语法树，使本模块不必等 frontmatter 语法扩展先合入，普通调用方始终按一个参数使用。
 */
export function buildOutline(content: string, parser: MarkdownParser = markdownParser): OutlineItem[] {
  if (!content.trim()) return [];
  const items: OutlineItem[] = [];
  collectHeadings(parser.parse(content).topNode, content, items);
  return items;
}
