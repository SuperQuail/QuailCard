import { createNoteFile, deleteFolder, deleteNoteFile, selectNote } from "../services/appState";
import { renameFolder, renameNoteFile } from "../services/stores/noteStore";
import { permanentDelete } from "../services/stores/uiStore";
import type { NoteMove } from "../domain/notePaths";
import { resolveError, resolveErrorCode } from "../utils/errorMessage";

/** 删除确认请求：展示层只渲染文案，具体决策（回收站 / 永久删除）由用例层决定。 */
export interface DeleteConfirmRequest {
  title: string;
  body: string;
  confirmLabel: string;
}

/** 删除目标：文件树多选与单项删除共用同一形状。 */
export interface DeleteTarget {
  kind: "folder" | "note";
  path: string;
}

/** 删除进度：回收站与永久删除分别计数，混合场景的提示才不会误导。 */
interface DeleteProgress {
  trashed: number;
  purged: number;
}

/** 壳层笔记操作用例的依赖：确认回调由壳层注入，用例本身不持有界面状态。 */
export interface NoteActionOptions {
  showToast: (message: string) => void;
  /** 危险操作确认；未注入时永久删除一律中止，绝不静默执行无法撤销的删除。 */
  confirm?: (request: DeleteConfirmRequest) => Promise<boolean>;
  onNoteOpened?: () => void;
}

/**
 * 壳层笔记操作用例：选笔记（窄屏收起抽屉）、快速捕获建笔记、批量删除、批量移动。
 * 只做 UI 反馈与跨域编排，业务在 store / 编排层。
 */
export function useNoteActions(options: NoteActionOptions) {
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

  /** 删除单项：permanent 必填，保证每次删除都有明确策略。 */
  async function deleteEntry(item: DeleteTarget, permanent: boolean): Promise<void> {
    if (item.kind === "folder") {
      await deleteFolder(item.path, permanent);
    } else {
      await deleteNoteFile(item.path, permanent);
    }
  }

  /** 删除项显示名：确认文案只呈现用户能认出的那一层名称。 */
  function entryName(item: DeleteTarget): string {
    const name = item.path.split("/").pop() ?? item.path;
    return item.kind === "folder" ? `文件夹「${name}」` : `笔记「${name.replace(/\.md$/i, "")}」`;
  }

  /** 永久删除确认正文：列出前几项名称并写明不可恢复。 */
  function permanentDeleteBody(items: DeleteTarget[]): string {
    const names = items.slice(0, 3).map(entryName).join("、");
    const rest = items.length > 3 ? ` 等 ${items.length} 项` : "";
    return `即将永久删除 ${items.length} 项：${names}${rest}。此操作不可恢复。`;
  }

  /** 等待用户选择；未注入确认回调时按取消处理，避免无法撤销的删除。 */
  async function askConfirm(request: DeleteConfirmRequest): Promise<boolean> {
    return options.confirm ? options.confirm(request) : false;
  }

  /** 回收站不可用时的二次确认：询问是否改为永久删除。 */
  function askPermanentFallback(): Promise<boolean> {
    return askConfirm({
      title: "无法移到回收站",
      body: "无法移到回收站，是否改为永久删除？",
      confirmLabel: "永久删除",
    });
  }

  /** 删除完成提示：区分回收站与永久删除，混合时分别计数。 */
  function reportDeleted(progress: DeleteProgress): void {
    if (progress.purged === 0) {
      options.showToast(`已移到回收站 ${progress.trashed} 项`);
      return;
    }
    if (progress.trashed === 0) {
      options.showToast(`已永久删除 ${progress.purged} 项`);
      return;
    }
    options.showToast(`已移到回收站 ${progress.trashed} 项，永久删除 ${progress.purged} 项`);
  }

  /**
   * 逐项删除：回收站失败（TRASH_UNAVAILABLE）时询问一次，
   * 确认后把当前项及后续未处理项切换为永久删除；其他错误按原有语义即停。
   *
   * 返回是否全部处理完成，调用方据此决定是否给出完成提示。
   */
  async function runDeletion(items: DeleteTarget[], permanent: boolean, progress: DeleteProgress): Promise<boolean> {
    let usePermanent = permanent;
    for (let index = 0; index < items.length; index += 1) {
      try {
        await deleteEntry(items[index], usePermanent);
        if (usePermanent) {
          progress.purged += 1;
        } else {
          progress.trashed += 1;
        }
      } catch (error) {
        if (!usePermanent && resolveErrorCode(error) === "TRASH_UNAVAILABLE") {
          if (!(await askPermanentFallback())) {
            // 取消后不再处理剩余项，明确告知已经处理了多少。
            options.showToast(`已处理 ${progress.trashed + progress.purged} 项，其余未删除`);
            return false;
          }
          usePermanent = true;
          // 当前项尚未删除成功，回退索引后以永久模式重试。
          index -= 1;
          continue;
        }
        options.showToast(resolveError(error));
        return false;
      }
    }
    return true;
  }

  /** 批量删除选中的笔记与文件夹：删除策略由设置与回收站可用性共同决定。 */
  async function handleDeleteSelection(items: DeleteTarget[]): Promise<void> {
    if (items.length === 0) {
      return;
    }
    const permanent = permanentDelete.value;
    if (permanent) {
      const confirmed = await askConfirm({
        title: "永久删除",
        body: permanentDeleteBody(items),
        confirmLabel: "永久删除",
      });
      // 取消时一个后端调用都不发，避免任何不可逆操作。
      if (!confirmed) {
        return;
      }
    }
    const progress: DeleteProgress = { trashed: 0, purged: 0 };
    if (await runDeletion(items, permanent, progress)) {
      reportDeleted(progress);
    }
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
