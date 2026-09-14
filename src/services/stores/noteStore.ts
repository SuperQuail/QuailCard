import { ref } from "vue";
import * as backend from "../../api/backend";
import type { NoteSummary, SearchResult } from "../../domain/types";
import { errorMessage } from "./uiStore";
import { refreshStats } from "./reviewStore";
import { createNotePersistence } from "../notePersistence";
import { remapNotePath, type NotePathChange } from "../../domain/notePaths";

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
export const noteOperationBusy = ref(false);
export const lastNotePathChange = ref<NotePathChange | null>(null);
let selectionRevision = 0;
let listRevision = 0;

/** 成功写入只发布元数据，不用旧请求的正文覆盖最新输入。 */
export const notePersistence = createNotePersistence(backend.writeNote, (path, _content, mtime) => {
  if (activeNotePath.value === path) {
    activeNoteMtime.value = mtime;
    savedAt.value = Date.now();
  }
  const summary = notes.value.find((note) => note.path === path);
  if (summary) summary.mtime = mtime;
});

/** 编辑器每次事务立即交出草稿，防抖与错误状态由保存服务维护。 */
export function updateNoteDraft(path: string, content: string): void {
  notePersistence.update(path, content);
  if (activeNotePath.value === path) activeNoteContent.value = content;
}

/** 重新加载笔记摘要与统计。 */
export async function refreshNotes(): Promise<void> {
  const revision = ++listRevision;
  const nextNotes = await backend.listNotes();
  if (revision === listRevision) notes.value = nextNotes;
  void refreshStats().catch((error) => { errorMessage.value = backend.resolveErrorMessage(error); });
}

/** 打开一篇笔记：加载正文，外部修改过时先重建索引。 */
export async function selectNote(path: string): Promise<void> {
  if (noteOperationBusy.value) return;
  const revision = ++selectionRevision;
  errorMessage.value = "";
  try {
    const readRevision = notePersistence.states.get(path)?.revision ?? 0;
    const file = await backend.readNote(path);
    if (revision !== selectionRevision) return;
    // 后台生成的新文件还没有摘要；编辑器依赖摘要身份，必须先补索引再发布选中状态。
    const summary = notes.value.find((note) => note.path === path);
    if (!summary || summary.mtime !== file.mtime) {
      await backend.syncNoteIndex(path);
      if (revision !== selectionRevision) return;
      await refreshNotes();
    }
    if (revision !== selectionRevision) return;
    activeNotePath.value = path;
    activeNoteContent.value = notePersistence.register(path, file.content, readRevision);
    activeNoteMtime.value = file.mtime;
    savedAt.value = Date.now();
  } catch (error) {
    if (revision === selectionRevision) errorMessage.value = backend.resolveErrorMessage(error);
  }
}

/** 保存笔记正文并同步摘要 mtime。 */
export async function saveNoteContent(path: string, content: string): Promise<void> {
  updateNoteDraft(path, content);
  await notePersistence.flush(path);
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
  await renameEntry(oldPath, newPath, backend.renameNoteFile);
}

/** 删除笔记：若删的是当前笔记则回退到列表中另一篇。 */
export async function deleteNoteFile(path: string): Promise<void> {
  await notePersistence.flush(path);
  await backend.deleteNoteFile(path);
  notePersistence.remove(path);
  if (activeNotePath.value === path) {
    const fallback = notes.value.find((note) => note.path !== path);
    activeNotePath.value = fallback?.path ?? null;
    activeNoteContent.value = "";
  }
  await refreshNotes();
}

/** 重命名文件夹：同步记住的空文件夹与当前笔记路径。 */
export async function renameFolder(oldPath: string, newPath: string): Promise<void> {
  await renameEntry(oldPath, newPath, backend.renameFolder);
}

/** 路径操作必须先保存；提交成功立即发布名称，刷新失败不能伪装成改名失败。 */
async function renameEntry(oldPath: string, newPath: string, rename: typeof backend.renameFolder): Promise<void> {
  if (oldPath === newPath) return;
  if (noteOperationBusy.value) throw new Error("正在处理文件操作，请稍后重试");
  noteOperationBusy.value = true;
  try {
    await notePersistence.flushAll();
    const renamed = await rename(oldPath, newPath);
    ++listRevision;
    ++selectionRevision;
    const remap = (path: string): string => remapNotePath(path, oldPath, renamed);
    notePersistence.rename(oldPath, renamed);
    notes.value = notes.value.map((note) => {
      const path = remap(note.path);
      return { ...note, path, title: path.split("/").pop()!.replace(/\.md$/i, "") };
    });
    extraFolders.value = extraFolders.value.map(remap);
    if (activeNotePath.value) activeNotePath.value = remap(activeNotePath.value);
    lastNotePathChange.value = { oldPath, newPath: renamed };
    try { await refreshNotes(); } catch (error) { errorMessage.value = backend.resolveErrorMessage(error); }
  } finally {
    noteOperationBusy.value = false;
  }
}

/** 删除文件夹及其笔记：当前笔记受影响时清空编辑区。 */
export async function deleteFolder(path: string): Promise<void> {
  await notePersistence.flushAll();
  await backend.deleteFolder(path);
  notePersistence.remove(path);
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
export async function reloadActiveContent(coordinated = false): Promise<void> {
  if (!activeNotePath.value) {
    return;
  }
  const path = activeNotePath.value;
  const readRevision = notePersistence.states.get(path)?.revision ?? 0;
  const file = await backend.readNote(path);
  if (activeNotePath.value !== path || (noteOperationBusy.value && !coordinated)) return;
  activeNoteContent.value = notePersistence.register(path, file.content, readRevision);
  activeNoteMtime.value = file.mtime;
}

/** 关闭最后一个标签仅清空当前选择；取消迟到读取，保留文件列表与保存服务。 */
export function clearActiveNote(): void {
  ++selectionRevision;
  activeNotePath.value = null;
  activeNoteContent.value = "";
  activeNoteMtime.value = 0;
  savedAt.value = 0;
}

/** 离开 Vault 时清空笔记域状态。 */
export function resetForVaultLeave(): void {
  ++selectionRevision;
  ++listRevision;
  notePersistence.remove();
  activeNotePath.value = null;
  activeNoteContent.value = "";
  notes.value = [];
  extraFolders.value = [];
  lastNotePathChange.value = null;
}
