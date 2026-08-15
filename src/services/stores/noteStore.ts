import { ref } from "vue";
import * as backend from "../../api/backend";
import type { NoteSummary, SearchResult } from "../../domain/types";
import { errorMessage } from "./uiStore";
import { refreshStats } from "./reviewStore";

/**
 * 笔记域 store：笔记列表、文件夹与当前打开的笔记。
 * 笔记增删改后同步刷新统计（原 refreshNotes 语义）；卡片加载由 cardStore 负责。
 */
export const notes = ref<NoteSummary[]>([]);
/** 尚未包含任何笔记的文件夹（Vault 扫描不到，需要前端记忆）。 */
export const extraFolders = ref<string[]>([]);
export const activeNotePath = ref<string | null>(null);
export const activeNoteContent = ref("");
export const activeNoteMtime = ref(0);
export const savedAt = ref(0);

/** 重新加载笔记摘要与统计。 */
export async function refreshNotes(): Promise<void> {
  const [nextNotes] = await Promise.all([backend.listNotes(), refreshStats()]);
  notes.value = nextNotes;
}

/** 打开一篇笔记：加载正文，外部修改过时先重建索引。 */
export async function selectNote(path: string): Promise<void> {
  activeNotePath.value = path;
  errorMessage.value = "";
  try {
    const file = await backend.readNote(path);
    // 检测外部编辑器修改：文件 mtime 与索引不一致时重建该笔记索引。
    const summary = notes.value.find((note) => note.path === path);
    if (summary && summary.mtime !== file.mtime) {
      await backend.syncNoteIndex(path);
      await refreshNotes();
    }
    activeNoteContent.value = file.content;
    activeNoteMtime.value = file.mtime;
    savedAt.value = Date.now();
  } catch (error) {
    errorMessage.value = backend.resolveErrorMessage(error);
  }
}

/** 保存笔记正文并同步摘要 mtime。 */
export async function saveNoteContent(path: string, content: string): Promise<void> {
  const mtime = await backend.writeNote(path, content);
  activeNoteMtime.value = mtime;
  activeNoteContent.value = content;
  savedAt.value = Date.now();
  const summary = notes.value.find((note) => note.path === path);
  if (summary) {
    summary.mtime = mtime;
  }
}

/** 新建笔记文件并打开（卡片加载由编排层协调 cardStore）。 */
export async function createNoteFile(folder: string, title: string, body: string): Promise<void> {
  const file = await backend.createNoteFile(folder, title);
  if (body.trim()) {
    await backend.writeNote(file.path, `# ${title}\n\n${body.trim()}\n`);
  }
  await refreshNotes();
  await selectNote(file.path);
}

/** 新建文件夹并记住（Vault 扫描不到空文件夹）。 */
export async function createFolder(path: string): Promise<void> {
  await backend.createFolder(path);
  if (!extraFolders.value.includes(path)) {
    extraFolders.value.push(path);
  }
}

/** 重命名笔记并刷新列表。 */
export async function renameNoteFile(oldPath: string, newPath: string): Promise<void> {
  const renamed = await backend.renameNoteFile(oldPath, newPath);
  if (activeNotePath.value === oldPath) {
    activeNotePath.value = renamed;
  }
  await refreshNotes();
}

/** 删除笔记：若删的是当前笔记则回退到列表中另一篇。 */
export async function deleteNoteFile(path: string): Promise<void> {
  await backend.deleteNoteFile(path);
  if (activeNotePath.value === path) {
    const fallback = notes.value.find((note) => note.path !== path);
    activeNotePath.value = fallback?.path ?? null;
    activeNoteContent.value = "";
  }
  await refreshNotes();
}

/** 重命名文件夹：同步记住的空文件夹与当前笔记路径。 */
export async function renameFolder(oldPath: string, newPath: string): Promise<void> {
  await backend.renameFolder(oldPath, newPath);
  extraFolders.value = extraFolders.value.map((folder) => {
    if (folder === oldPath) {
      return newPath;
    }
    if (folder.startsWith(`${oldPath}/`)) {
      return `${newPath}/${folder.slice(oldPath.length + 1)}`;
    }
    return folder;
  });
  if (activeNotePath.value?.startsWith(`${oldPath}/`)) {
    activeNotePath.value = `${newPath}/${activeNotePath.value.slice(oldPath.length + 1)}`;
  }
  await refreshNotes();
}

/** 删除文件夹及其笔记：当前笔记受影响时清空编辑区。 */
export async function deleteFolder(path: string): Promise<void> {
  await backend.deleteFolder(path);
  extraFolders.value = extraFolders.value.filter((folder) => folder !== path && !folder.startsWith(`${path}/`));
  if (activeNotePath.value === path || activeNotePath.value?.startsWith(`${path}/`)) {
    activeNotePath.value = null;
    activeNoteContent.value = "";
  }
  await refreshNotes();
}

/** 全文搜索。 */
export function search(query: string): Promise<SearchResult> {
  return backend.search(query);
}

/** 按路径查找笔记摘要。 */
export function findNote(path: string | null): NoteSummary | null {
  return notes.value.find((note) => note.path === path) ?? null;
}

/** 重扫后以磁盘为准重载当前笔记内容。 */
export async function reloadActiveContent(): Promise<void> {
  if (!activeNotePath.value) {
    return;
  }
  const file = await backend.readNote(activeNotePath.value);
  activeNoteContent.value = file.content;
  activeNoteMtime.value = file.mtime;
}

/** 离开 Vault 时清空笔记域状态。 */
export function resetForVaultLeave(): void {
  activeNotePath.value = null;
  activeNoteContent.value = "";
  notes.value = [];
}
