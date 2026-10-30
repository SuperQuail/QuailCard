import { syntaxTree } from "@codemirror/language";
import { StateField, type EditorState, type Extension, type Range } from "@codemirror/state";
import { Decoration, type DecorationSet, EditorView, WidgetType } from "@codemirror/view";
import { readingMode, readingModeChanged } from "../editor/readingMode";
import { FRONTMATTER } from "./frontmatter";

/** frontmatter 的一行属性；无法识别成键值的行按原文放在 text 里。 */
export interface FrontmatterEntry {
  key: string;
  /** 非 chip 键的原始值文本（列表会保留原样）。 */
  text: string;
  /** tags/aliases 这类可展开成 chip 的键；其他键为 null。 */
  chips: string[] | null;
}

/** 展示成 chip 的常用键；大小写不敏感，未知键一律按普通键值展示。 */
const CHIP_KEYS = new Set(["tags", "tag", "aliases", "alias"]);

/** 解析 YAML 内联列表与逗号分隔值，去掉常见引号。 */
function parseListValue(value: string): string[] {
  const trimmed = value.trim();
  if (!trimmed) return [];
  const body = trimmed.startsWith("[") && trimmed.endsWith("]") ? trimmed.slice(1, -1) : trimmed;
  return body
    .split(/[,\uFF0C]/)
    .map((item) => item.trim().replace(/^["']|["']$/g, "").trim())
    .filter(Boolean);
}

/** 块列表行（`- 项`）判断，用于 tags: 后面换行写列表的常见写法。 */
function listItem(line: string): string | null {
  const match = /^[-*]\s+(.*)$/.exec(line.trim());
  return match ? match[1]!.trim() : null;
}

/**
 * 把 frontmatter 原文解析成属性列表。
 * 契约：只做展示所需的宽松解析——认不出的行按原文输出，绝不让显示侧因为
 * 一段非标准 YAML 而空白；标签与别名同时接受 [a, b]、a, b 与块列表三种写法。
 */
export function parseFrontmatterEntries(source: string): FrontmatterEntry[] {
  const lines = source.split(/\r?\n/);
  // 去掉首行 --- 与末行闭合 ---；未闭合的文档不该走到这里，保底也不展示定界行。
  const body = lines.length > 2 ? lines.slice(1, lines.length - 1) : [];
  const entries: FrontmatterEntry[] = [];
  for (let index = 0; index < body.length; index++) {
    const line = body[index]!.trim();
    if (!line || line.startsWith("#")) continue;
    const match = /^([^:]+?)\s*:\s*(.*)$/.exec(line);
    if (!match) {
      entries.push({ key: "", text: line, chips: null });
      continue;
    }
    const key = match[1]!.trim();
    const value = match[2]!.trim();
    const chipKey = CHIP_KEYS.has(key.toLowerCase());
    if (chipKey) {
      const items = parseListValue(value);
      // 值留空时继续吃后面的块列表行，父键与列表项合并成一个属性。
      while (!value && index + 1 < body.length) {
        const item = listItem(body[index + 1]!);
        if (item === null) break;
        index++;
        if (item) items.push(item);
      }
      entries.push({ key, text: value, chips: items });
      continue;
    }
    entries.push({ key, text: value, chips: null });
  }
  return entries;
}

/** 阅读态属性块：标签/别名用 chip，其余键值原样展示。 */
class FrontmatterWidget extends WidgetType {
  constructor(private readonly source: string) {
    super();
  }

  /** 内容不变就不重建 DOM，避免每次光标移动都重排整块。 */
  eq(other: FrontmatterWidget): boolean {
    return other.source === this.source;
  }

  /** 全部用 textContent 构造，笔记里的 HTML/YAML 标签不会被执行。 */
  toDOM(): HTMLElement {
    const section = document.createElement("section");
    section.className = "qc-frontmatter-props";
    const header = section.appendChild(document.createElement("div"));
    header.className = "qc-frontmatter-props-header";
    header.textContent = "属性";
    const entries = parseFrontmatterEntries(this.source);
    if (entries.length === 0) {
      const empty = section.appendChild(document.createElement("div"));
      empty.className = "qc-frontmatter-empty";
      empty.textContent = "（没有可展示的属性）";
      return section;
    }
    for (const entry of entries) {
      const row = section.appendChild(document.createElement("div"));
      row.className = "qc-frontmatter-prop";
      const key = row.appendChild(document.createElement("span"));
      key.className = "qc-frontmatter-key";
      key.textContent = entry.key || "—";
      const value = row.appendChild(document.createElement("span"));
      value.className = "qc-frontmatter-value";
      if (entry.chips && entry.chips.length > 0) {
        for (const chip of entry.chips) {
          const item = value.appendChild(document.createElement("span"));
          item.className = "qc-frontmatter-chip";
          item.textContent = chip;
        }
      } else {
        value.textContent = entry.text;
      }
    }
    return section;
  }

  /** 属性块是只读展示，点击不应把光标塞进被替换掉的源码。 */
  ignoreEvent(): boolean {
    return true;
  }
}

/** 编辑态给 frontmatter 每行挂弱化样式；阅读态整块替换成属性块。 */
function buildDecorations(state: EditorState): DecorationSet {
  const ranges: Range<Decoration>[] = [];
  const reading = state.facet(readingMode);
  syntaxTree(state).iterate({
    enter(node) {
      if (node.name !== FRONTMATTER) return;
      if (reading) {
        ranges.push(Decoration.replace({
          widget: new FrontmatterWidget(state.doc.sliceString(node.from, node.to)),
          block: true,
        }).range(node.from, node.to));
        return false;
      }
      const first = state.doc.lineAt(node.from).number;
      const last = state.doc.lineAt(node.to).number;
      for (let number = first; number <= last; number++) {
        ranges.push(Decoration.line({ class: "qc-frontmatter-line" }).range(state.doc.line(number).from));
      }
      return false;
    },
  });
  return Decoration.set(ranges, true);
}

/** 阅读态属性块与编辑态样式的主题；用全局 CSS 变量保持深浅色一致。 */
const frontmatterTheme = EditorView.theme({
  ".qc-frontmatter-line": {
    color: "var(--qc-ink-3)",
    fontFamily: "var(--font-mono)",
    fontSize: "0.86em",
  },
  ".qc-frontmatter-props": {
    margin: "0 0 18px",
    padding: "10px 14px",
    border: "1px solid var(--qc-hairline)",
    borderRadius: "8px",
    backgroundColor: "var(--qc-bg-side)",
    color: "var(--qc-ink-2)",
    fontFamily: "var(--font-ui)",
    fontSize: "12px",
    lineHeight: "1.7",
  },
  ".qc-frontmatter-props-header": {
    marginBottom: "4px",
    color: "var(--qc-ink-3)",
    letterSpacing: "0.08em",
  },
  ".qc-frontmatter-prop": {
    display: "flex",
    gap: "8px",
    alignItems: "baseline",
  },
  ".qc-frontmatter-key": {
    minWidth: "64px",
    color: "var(--qc-ink-3)",
    fontFamily: "var(--font-mono)",
  },
  ".qc-frontmatter-value": {
    color: "var(--qc-ink-2)",
    fontFamily: "var(--font-mono)",
    whiteSpace: "pre-wrap",
    wordBreak: "break-word",
  },
  ".qc-frontmatter-chip": {
    display: "inline-block",
    margin: "0 4px 2px 0",
    padding: "0 6px",
    border: "1px solid var(--qc-hairline)",
    borderRadius: "999px",
    backgroundColor: "var(--qc-bg-paper)",
    color: "var(--qc-ink-2)",
  },
  ".qc-frontmatter-empty": {
    color: "var(--qc-ink-3)",
  },
});

/**
 * 阅读态的整块替换属于块级装饰，只能由 StateField 提供（ViewPlugin 不允许块装饰）；
 * 编辑态的行样式与它放在同一个字段里，保证两种模式切换时原子替换。
 */
const frontmatterDecorations = StateField.define<DecorationSet>({
  create: buildDecorations,
  /** 文档、阅读模式或后台解析变化后重新判断折叠范围。 */
  update(value, transaction) {
    return transaction.docChanged
      || readingModeChanged(transaction.startState, transaction.state)
      || syntaxTree(transaction.startState) !== syntaxTree(transaction.state)
      ? buildDecorations(transaction.state)
      : value;
  },
  provide: (field) => EditorView.decorations.from(field),
});

/** frontmatter 视图扩展：编辑态弱化显示原文，阅读态折叠成"属性"块。 */
export function frontmatterExtension(): Extension {
  return [frontmatterDecorations, frontmatterTheme];
}
