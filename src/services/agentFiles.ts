import { nextTick } from "vue";
import * as backend from "../api/backend";
import * as agentApi from "../api/agent";
import { activeNotePath, activeNoteContent, notes, noteOperationBusy, notePersistence, refreshNotes, reloadActiveContent } from "./stores/noteStore";

/** 与编辑器已有自动保存队列协调，短暂只读只覆盖真实写入过程。 */
export async function coordinateAgentWrite(action: () => Promise<void>): Promise<void> {
  if (noteOperationBusy.value) throw new Error("正在保存笔记，请稍后继续");
  noteOperationBusy.value = true;
  try {
    await nextTick();
    await notePersistence.flushAll();
    await action();
    await backend.rescanVault();
    await refreshNotes();
    if (activeNotePath.value) {
      const path = activeNotePath.value;
      if (!notes.value.some(n => n.path === path)) { notePersistence.remove(path); activeNotePath.value = null; activeNoteContent.value = ""; }
      else await reloadActiveContent(true);
    }
  } finally { noteOperationBusy.value = false; }
}

/** 撤销以持久化操作为目标，不把前端展示内容作为写入来源。 */
export async function undoAgentChange(id: string): Promise<void> {
  await coordinateAgentWrite(async () => { await agentApi.undo(id); });
}
