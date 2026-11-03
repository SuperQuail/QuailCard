import type { Backlink, DictionaryEntry, NoteCard, NoteFile, NoteSummary, RenameOutcome, SearchResult } from "../../domain/types";
import { documentKindOf, isWordGroupPath, stripDocumentExtension } from "../../domain/documentKind";
import { createEntry, parseWordGroup, serializeWordGroup } from "../../domain/wordGroup";
import { cards, makeCard, notes, type MockCard } from "./state";

/**
 * 笔记域演示处理：文件树、笔记读写、文件夹操作与全文搜索。
 *
 * 说明：全部为内存操作，仅演示 UI 交互；不做真实文件系统读写（真实实现
 * 在 Rust 后端，前端本就不直接碰文件）。不实现任何业务规则。
 */

/** 从路径推导标题：取最后一个路径段并去掉文档扩展名（.md / .qword）。 */
export function titleFromPath(path: string): string {
  return stripDocumentExtension(path.split("/").pop() ?? path);
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
      // 单词组不参与标签与双链：演示摘要同样不解析 JSON 正文里的标签。
      tagsJson: isWordGroupPath(path) ? "[]" : JSON.stringify(tagsFromContent(note.content)),
      aliases: [],
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

/**
 * 写入笔记正文（演示级，统一换行符）。
 * 单词组额外按页面同步演示卡片，模拟"保存时物化"；解析失败只跳过同步，不复刻后端报错。
 */
export function writeNote(path: string, content: string): number {
  const now = Math.floor(Date.now() / 1000);
  const normalized = content.replace(/\r\n/g, "\n");
  notes.set(path, { content: normalized, mtime: now });
  if (documentKindOf(path) === "wordGroup") {
    syncWordGroupCards(path, normalized);
  }
  return now;
}

/**
 * 演示级卡片同步：完整页新建/更新 vocabulary 卡（id 就是页面 id），
 * 草稿页不新建、已有同 id 卡片原样保留，页面消失才删卡。不复制后端的校验与冲突策略。
 */
function syncWordGroupCards(path: string, content: string): void {
  const result = parseWordGroup(content);
  if (!result.ok) return;
  const existing = new Map<string, MockCard>();
  for (const card of cards.values()) {
    if (card.notePath === path) existing.set(card.id, card);
  }
  const kept = new Set<string>();
  result.document.words.forEach((page, index) => {
    const complete = page.word.trim().length > 0 && page.meaning.trim().length > 0;
    const card = existing.get(page.id);
    if (!complete) {
      // 草稿页：不新建，已有卡片原样保留（避免编辑中间态丢复习历史）。
      if (card) kept.add(card.id);
      return;
    }
    kept.add(page.id);
    if (card) {
      card.front = page.meaning;
      card.back = page.word;
      card.detail = page.phonetic;
      card.example = page.example;
      card.position = index;
      return;
    }
    // 同 id 已属于其它笔记时不抢占，演示环境直接跳过。
    if (cards.has(page.id)) return;
    const created = makeCard(path, "vocabulary", page.meaning, page.word, page.phonetic, page.example, index);
    created.id = page.id;
    cards.set(created.id, created);
  });
  for (const id of existing.keys()) {
    if (!kept.has(id)) cards.delete(id);
  }
}

/** 新建单词组文件（演示级）：写入含一个空页的 JSON，不产生 .md。 */
export function createWordGroup(folder: string, rawTitle: string): NoteFile {
  const now = Math.floor(Date.now() / 1000);
  const title = stripDocumentExtension(rawTitle.endsWith(".qword") ? rawTitle : `${rawTitle}.qword`);
  const path = folder ? `${folder}/${title}.qword` : `${title}.qword`;
  if (notes.has(path)) {
    throw new Error("同名单词组已存在");
  }
  const content = serializeWordGroup({ formatVersion: 1, words: [createEntry()] });
  notes.set(path, { content, mtime: now });
  return { path, title, content, mtime: now };
}

/** 演示用小词表：只覆盖种子词与常见示例，未收录返回 null（与真实 ECDICT 同形）。 */
const MOCK_DICTIONARY: Readonly<Record<string, Omit<DictionaryEntry, "word">>> = {
  ephemeral: { phonetic: "/ɪˈfemərəl/", translation: "adj. 短暂的；转瞬即逝的", definition: "lasting for a very short time", pos: "adj.", collins: 1, oxford: 1, bnc: 12000, frq: 9800, exchange: "s:ephemerals" },
  contemplate: { phonetic: "/ˈkɑːntəmpleɪt/", translation: "v. 深入思考；仔细考虑", definition: "think about something deeply", pos: "v.", collins: 2, oxford: 1, bnc: 7000, frq: 6400, exchange: "p:contemplated" },
  resilient: { phonetic: "/rɪˈzɪliənt/", translation: "adj. 有复原力的；能迅速恢复的", definition: "able to recover quickly", pos: "adj.", collins: 2, oxford: 1, bnc: 9000, frq: 8200, exchange: "s:resilients" },
  ubiquitous: { phonetic: "/juːˈbɪkwɪtəs/", translation: "adj. 无处不在的", definition: "present everywhere", pos: "adj.", collins: 1, oxford: 0, bnc: 15000, frq: 13000, exchange: "" },
  meticulous: { phonetic: "/məˈtɪkjələs/", translation: "adj. 一丝不苟的", definition: "showing great attention to detail", pos: "adj.", collins: 1, oxford: 0, bnc: 14000, frq: 12000, exchange: "" },
};

/** 演示级词典查询：大小写与首尾空白不敏感，未收录返回 null。 */
export function lookupDictionaryWord(rawWord: string): DictionaryEntry | null {
  const word = rawWord.trim().toLowerCase();
  const hit = MOCK_DICTIONARY[word];
  return hit ? { word, ...hit } : null;
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

/** 重命名笔记并同步其下卡片的 notePath；演示环境不复刻链接改写，故 updatedNotes 恒为空。 */
export function renameNoteFile(oldPath: string, newPath: string): RenameOutcome {
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
  return { path: newPath, updatedNotes: [], failedNotes: [] };
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
export function renameFolder(oldPath: string, newPath: string): RenameOutcome {
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
  return { path: newPath, updatedNotes: [], failedNotes: [] };
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

/**
 * 反链查询：演示环境只做 [[目标]] 的字面匹配，不实现同名/别名的解析优先级。
 * 真实规则在 Rust 后端，前端演示不需要复刻。
 */
export function backlinks(path: string): Backlink[] {
  const title = titleFromPath(path).toLowerCase();
  const result: Backlink[] = [];
  const needle = "[[" + title;
  for (const [sourcePath, note] of notes) {
    if (sourcePath === path) continue;
    note.content.split(/\r?\n/).forEach((line, index) => {
      if (line.toLowerCase().includes(needle)) {
        result.push({ sourcePath, sourceTitle: titleFromPath(sourcePath), line: index + 1, snippet: line.trim() });
      }
    });
  }
  return result;
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
    source: card.source ?? null,
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
