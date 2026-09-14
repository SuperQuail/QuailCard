import { createNoteFile, deleteFolder, deleteNoteFile, selectNote } from "../services/appState";
import { renameFolder, renameNoteFile } from "../services/stores/noteStore";
import type { NoteMove } from "../domain/notePaths";
import { resolveError } from "../utils/errorMessage";

/**
 * 壳层笔记操作用例：选笔记（窄屏收起抽屉）、快速捕获建笔记、批量删除、批量移动。
 * 只做 UI 反馈与跨域编排，业务在 store / 编排层。
 */
export function useNoteActions(options: { showToast: (message: string) => void; onNoteOpened?: () => void }) {
  /** 打开笔记并触发壳层副作用（窄窗口收起抽屉）。 */
  async function handleSelectNote(path: string): Promise<void> {
    await selectNote(path);
    options.onNoteOpened?.();
  }

  /** 快速捕获创建笔记，成功后关闭入口由调用方处理。 */
  async function handleQuickCapture(title: string, folder: string, body: string): Promise<boolean> {
    try {
      await createNoteFile(folder, title, body);
      return true;
    } catch (error) {
      options.showToast(resolveError(error));
      return false;
    }
  }

  /** 批量删除选中的笔记与文件夹，遇错即停并反馈。 */
  async function handleDeleteSelection(items: Array<{ kind: "folder" | "note"; path: string }>): Promise<void> {
    let deleted = 0;
    for (const item of items) {
      try {
        if (item.kind === "folder") {
          await deleteFolder(item.path);
        } else {
          await deleteNoteFile(item.path);
        }
        deleted += 1;
      } catch (error) {
        options.showToast(resolveError(error));
        return;
      }
    }
    options.showToast(`已删除 ${deleted} 项`);
  }

  /** 批量移动拖拽落下的笔记与文件夹：逐项执行，成功后提示总数。 */
  async function handleMoveEntries(moves: NoteMove[]): Promise<void> {
    if (moves.length === 0) return;
    let moved = 0;
    for (const move of moves) {
      try {
        if (move.kind === "folder") {
          await renameFolder(move.from, move.to);
        } else {
          await renameNoteFile(move.from, move.to);
        }
        moved += 1;
      } catch (error) {
        // 已成功的条目保持已移动状态，失败原因单独提示，避免误导为整体失败。
        options.showToast(moved > 0 ? `已移动 ${moved} 项，其余失败：${resolveError(error)}` : resolveError(error));
        return;
      }
    }
    options.showToast(`已移动 ${moved} 项`);
  }

  return { handleSelectNote, handleQuickCapture, handleDeleteSelection, handleMoveEntries };
}
