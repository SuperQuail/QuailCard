/** 标题层级：与 Markdown 的六种标题一一对应。 */
export type HeadingLevel = 1 | 2 | 3 | 4 | 5 | 6;

/** 表格列对齐方式。 */
export type TableAlignment = "left" | "center" | "right";

/** 列表项：保留原始 Markdown 文本，行内格式与读音交给渲染端。 */
export interface NoteListItem {
  text: string;
  /** 嵌套层级，0 为顶层。 */
  depth: number;
  /** 该项所在列表是否为有序列表。 */
  ordered: boolean;
  /** 有序列表内的序号，从 1 开始；无序列表同样给出位置。 */
  index: number;
  /** 任务项是否已勾选；非任务项为 null。 */
  task: boolean | null;
}

/** 笔记正文块：显示侧与编辑器共享同一份解析结果。 */
export type NoteBlock =
  | { type: "heading"; level: HeadingLevel; text: string }
  | { type: "p"; text: string }
  | { type: "quote"; text: string }
  | { type: "code"; text: string; language: string }
  | { type: "list"; items: NoteListItem[] }
  | { type: "table"; header: string[]; rows: string[][]; alignments: TableAlignment[] }
  | { type: "hr" }
  | { type: "card"; text: string; cardId: string };

/** 大纲标题：只保留渲染与跳转需要的字段。 */
export interface NoteHeading {
  text: string;
  level: HeadingLevel;
}

/** 行内片段：显示端一律用文本插值，笔记里的 HTML 永远不会被执行。 */
export interface InlineSpan {
  text: string;
  kind?: "bold" | "italic" | "code" | "strike" | "link" | "math";
  /** kind 为 link 时的目标；是否可跳转由渲染端决定。 */
  href?: string;
  /** kind 为 math 时是否为单行块级公式（$$…$$）。 */
  display?: boolean;
}
