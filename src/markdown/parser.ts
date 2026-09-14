import { markdownLanguage } from "@codemirror/lang-markdown";
import type { MarkdownParser } from "@lezer/markdown";
import { mathExtension } from "./math";

/**
 * 唯一的 Markdown 解析器：显示侧与编辑器用的都是 `markdownLanguage` 这一份语法配置，
 * 只额外挂上同一个公式扩展实例，语法理解不会再两边漂移。
 * 它是 GFM + 下标 + 上标 + emoji 的语法树；显示侧只渲染其中一部分，
 * 未支持的节点一律按原文输出（例如 `12:30:45`、`^_^`、`5~10~20` 保持原样）。
 */
export const markdownParser: MarkdownParser = (markdownLanguage.parser as MarkdownParser).configure([mathExtension]);

/** 编辑器语言与显示侧共用的扩展：新增语法时只在这里登记一次。 */
export { mathExtension };

/** 语法树节点类型：随解析器推导，避免到处写 @lezer/common 的导入。 */
export type MarkdownNode = ReturnType<typeof markdownParser.parse>["topNode"];
