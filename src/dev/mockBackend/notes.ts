import type { NoteCard, NoteFile, NoteSummary, SearchResult } from "../../domain/types";
import { cards, notes, type MockCard } from "./state";

/**
 * 笔记域演示处理：文件树、笔记读写、文件夹操作与全文搜索。
 *
 * 说明：全部为内存操作，仅演示 UI 交互；不做真实文件系统读写（真实实现
 * 在 Rust 后端，前端本就不直接碰文件）。不实现任何业务规则。
 */

/** 从路径推导标题：取最后一个路径段并去掉 .md 后缀。 */
export function titleFromPath(path: string): string {
  return path.split("/").pop()?.replace(/\.md$/, "") ?? path;
}

/** 提取正文中以 # 开头的标签，用于笔记摘要展示。 */
export function tagsFromContent(content: string): string[] {
  const tags: string[] = [];
  for (const word of content.split(/\s+/)) {
    if (word.startsWith("#")) {
      const tag = word.replace(/[^\p{L}\p{N}_\-/]/gu, "");
      if (tag && !tags.includes(tag)) {
        tags.push(tag);
      }
    }
  }
  return tags;
}

/** 生成排序后的笔记摘要列表（演示级：按标题中文排序）。 */
export function noteSummaries(): NoteSummary[] {
  const result: NoteSummary[] = [];
  const now = Math.floor(Date.now() / 1000);
  for (const [path, note] of notes) {
    let cardCount = 0;
    let dueCount = 0;
    for (const card of cards.values()) {
      if (card.notePath === path) {
        cardCount += 1;
        if (card.dueAt <= now) {
          dueCount += 1;
        }
      }
    }
    result.push({
      path,
      title: titleFromPath(path),
      tagsJson: JSON.stringify(tagsFromContent(note.content)),
      cardCount,
      dueCount,
      mtime: note.mtime,
    });
  }
  return result.sort((left, right) => left.title.localeCompare(right.title, "zh-CN"));
}

/** 读取单条笔记内容（演示级）。 */
export function readNote(path: string): NoteFile {
  const note = notes.get(path);
  if (!note) {
    throw new Error("笔记文件不存在");
  }
  return { path, title: titleFromPath(path), content: note.content, mtime: note.mtime };
}

/** 写入笔记正文（演示级，统一换行符）。 */
export function writeNote(path: string, content: string): number {
  const now = Math.floor(Date.now() / 1000);
  notes.set(path, { content: content.replace(/\r\n/g, "\n"), mtime: now });
  return now;
}

/** 新建笔记文件（演示级）。 */
export function createNoteFile(folder: string, rawTitle: string): NoteFile {
  const now = Math.floor(Date.now() / 1000);
  const title = rawTitle.endsWith(".md") ? rawTitle : `${rawTitle}.md`;
  const path = folder ? `${folder}/${title}` : title;
  if (notes.has(path)) {
    throw new Error("同名笔记已存在");
  }
  const content = `# ${rawTitle}\n`;
  notes.set(path, { content, mtime: now });
  return { path, title: rawTitle, content, mtime: now };
}

/** 重命名笔记并同步其下卡片的 notePath。 */
export function renameNoteFile(oldPath: string, newPath: string): string {
  const note = notes.get(oldPath);
  if (!note) {
    throw new Error("笔记文件不存在");
  }
  notes.delete(oldPath);
  notes.set(newPath, note);
  for (const card of cards.values()) {
    if (card.notePath === oldPath) {
      card.notePath = newPath;
    }
  }
  return newPath;
}

/** 删除笔记及其下全部卡片。 */
export function deleteNoteFile(path: string): void {
  notes.delete(path);
  for (const [id, card] of [...cards.entries()]) {
    if (card.notePath === path) {
      cards.delete(id);
    }
  }
}

/** 重命名文件夹：把前缀匹配的笔记与卡片路径一并改前缀。 */
export function renameFolder(oldPath: string, newPath: string): string {
  for (const [path, note] of [...notes.entries()]) {
    if (path === oldPath || path.startsWith(`${oldPath}/`)) {
      const next = newPath + path.slice(oldPath.length);
      notes.delete(path);
      notes.set(next, note);
    }
  }
  for (const card of cards.values()) {
    if (card.notePath === oldPath || card.notePath.startsWith(`${oldPath}/`)) {
      card.notePath = newPath + card.notePath.slice(oldPath.length);
    }
  }
  return newPath;
}

/** 删除文件夹及其下全部内容。 */
export function deleteFolder(folder: string): void {
  for (const [path] of [...notes.entries()]) {
    if (path === folder || path.startsWith(`${folder}/`)) {
      notes.delete(path);
    }
  }
  for (const [id, card] of [...cards.entries()]) {
    if (card.notePath === folder || card.notePath.startsWith(`${folder}/`)) {
      cards.delete(id);
    }
  }
}

/** 全文搜索笔记与卡片（演示级：朴素包含匹配）。 */
export function search(query: string): SearchResult {
  const q = query.trim().toLowerCase();
  const result: SearchResult = { notes: [], cards: [] };
  if (!q) {
    return result;
  }
  for (const [path, note] of notes) {
    const index = note.content.toLowerCase().indexOf(q);
    if (index >= 0) {
      result.notes.push({
        path,
        title: titleFromPath(path),
        snippet: note.content.slice(Math.max(0, index - 20), index + q.length + 30),
      });
    }
  }
  for (const card of cards.values()) {
    const haystack = `${card.front} ${card.back}`.toLowerCase();
    if (haystack.includes(q)) {
      result.cards.push({ cardId: card.id, notePath: card.notePath, front: card.front, snippet: card.back });
    }
  }
  return result;
}

/** 将卡片摘要化为 NoteCard，供卡片面板消费。 */
export function toNoteCard(card: MockCard): NoteCard {
  return {
    id: card.id,
    notePath: card.notePath,
    sourceRef: card.sourceRef,
    kind: card.kind,
    front: card.front,
    back: card.back,
    detail: card.detail,
    example: card.example,
    aliases: card.aliases,
    rubricPoints: card.rubric,
    position: card.position,
    schedulerPhase: card.schedulerPhase,
    dueAt: card.dueAt,
    intervalDays: card.intervalDays,
    totalReviews: card.totalReviews,
    version: card.version,
  };
}
