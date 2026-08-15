/** 笔记正文块：编辑器渲染与序列化的统一结构。 */
export type NoteBlock =
  | { type: "h1"; text: string }
  | { type: "h2"; text: string }
  | { type: "h3"; text: string }
  | { type: "p"; text: string }
  | { type: "quote"; text: string }
  | { type: "code"; text: string }
  | { type: "list"; items: string[] }
  | { type: "hr" }
  | { type: "card"; text: string; cardId: string };

/** 从文件路径推导笔记标题。 */
export function titleFromPath(path: string): string {
  return path.split("/").pop()?.replace(/\.md$/i, "") ?? path;
}

/** 从块锚点中提取卡片 ID。 */
export function cardIdFromAnchor(anchor: string): string | null {
  const match = anchor.match(/\^qc-([\w-]+)/);
  return match ? match[1] : null;
}

/** 判断文本是否为 Markdown 标题行。 */
function parseHeading(line: string): { level: 1 | 2 | 3; text: string } | null {
  const match = line.match(/^(#{1,3})\s+(.+)$/);
  if (!match) {
    return null;
  }
  return { level: match[1].length as 1 | 2 | 3, text: match[2].trim() };
}

/** 将 Markdown 正文解析为结构化块。 */
export function parseMarkdown(content: string): NoteBlock[] {
  const blocks: NoteBlock[] = [];
  const lines = content.replace(/\r\n/g, "\n").split("\n");
  let index = 0;
  while (index < lines.length) {
    const line = lines[index];
    const trimmed = line.trim();

    // 代码块
    if (trimmed.startsWith("```")) {
      const codeLines: string[] = [];
      index += 1;
      while (index < lines.length && !lines[index].trim().startsWith("```")) {
        codeLines.push(lines[index]);
        index += 1;
      }
      index += 1;
      blocks.push({ type: "code", text: codeLines.join("\n") });
      continue;
    }

    if (trimmed === "") {
      index += 1;
      continue;
    }

    // 分隔线
    if (/^(-{3,}|\*{3,})$/.test(trimmed)) {
      blocks.push({ type: "hr" });
      index += 1;
      continue;
    }

    // 标题
    const heading = parseHeading(trimmed);
    if (heading) {
      blocks.push({ type: `h${heading.level}`, text: heading.text });
      index += 1;
      continue;
    }

    // 引用
    if (trimmed.startsWith(">")) {
      blocks.push({ type: "quote", text: trimmed.replace(/^>\s?/, "") });
      index += 1;
      continue;
    }

    // 列表
    if (/^[-*+]\s+/.test(trimmed)) {
      const items: string[] = [];
      while (index < lines.length && /^[-*+]\s+/.test(lines[index].trim())) {
        items.push(lines[index].trim().replace(/^[-*+]\s+/, ""));
        index += 1;
      }
      blocks.push({ type: "list", items });
      continue;
    }

    // 带块锚点的段落：识别为卡片标记
    const anchor = cardIdFromAnchor(trimmed);
    if (anchor) {
      const text = trimmed.replace(/\^qc-[\w-]+\s*$/, "").trim();
      blocks.push({ type: "card", text, cardId: anchor });
      index += 1;
      continue;
    }

    // 普通段落
    blocks.push({ type: "p", text: trimmed });
    index += 1;
  }
  return blocks;
}

/** 将结构化块序列化为 Markdown 正文。 */
export function serializeBlocks(blocks: NoteBlock[]): string {
  const lines: string[] = [];
  for (const block of blocks) {
    switch (block.type) {
      case "h1":
      case "h2":
      case "h3": {
        const level = Number(block.type.slice(1));
        lines.push(`${"#".repeat(level)} ${block.text}`, "");
        break;
      }
      case "p":
        lines.push(block.text, "");
        break;
      case "quote":
        lines.push(`> ${block.text}`, "");
        break;
      case "code":
        lines.push("```", block.text, "```", "");
        break;
      case "list":
        for (const item of block.items) {
          lines.push(`- ${item}`);
        }
        lines.push("");
        break;
      case "hr":
        lines.push("---", "");
        break;
      case "card":
        lines.push(`${block.text} ^qc-${block.cardId}`, "");
        break;
    }
  }
  return lines.join("\n").trimEnd() + "\n";
}

/** 在指定块中为选中文本追加卡片锚点，返回新块与正文。 */
export function attachAnchorToBlock(
  blocks: NoteBlock[],
  blockIndex: number,
  selectedText: string,
  cardId: string,
): NoteBlock[] {
  return blocks.map((block, index) => {
    if (index !== blockIndex || block.type === "card") {
      return block;
    }
    if (!("text" in block) || !block.text.includes(selectedText)) {
      return block;
    }
    return { ...block, text: block.text.replace(selectedText, `${selectedText} ^qc-${cardId}`) };
  });
}

/** 估算笔记正文字数。 */
export function countContentWords(content: string): number {
  return content.replace(/\s+/g, "").length;
}
