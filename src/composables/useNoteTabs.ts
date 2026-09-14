import { computed, getCurrentScope, nextTick, onScopeDispose, ref, watch, type ComputedRef, type Ref } from "vue";
import { remapNotePath, type NotePathChange } from "../domain/notePaths";
import type { NoteSummary } from "../domain/types";

/** 标签只记录成功激活过的路径；文件读写和选择仍由调用方负责。 */
export function useNoteTabs(options: {
  notes: Ref<NoteSummary[]>;
  activePath: Ref<string | null>;
  vaultPath: Ref<string | null>;
  pathChange: Ref<NotePathChange | null>;
  busy: Ref<boolean>;
  select: (path: string) => Promise<void>;
  flush: (path: string) => Promise<void>;
  clearActive: () => void;
  onError: (message: string) => void;
}): { tabs: ComputedRef<{ path: string; title: string }[]>; close: (path: string) => Promise<void>; closing: Ref<boolean> } {
  const paths = ref<string[]>([]);
  const closing = ref(false);
  let opened: string[] = [];
  let changes: NotePathChange[] = [];
  let identityRevision = 0;
  let focusRevision = 0;
  let disposed = false;

  /** 元数据变化不改变打开顺序，标题始终来自最新摘要。 */
  const tabs = computed(() => {
    const summaries = new Map(options.notes.value.map((note) => [note.path, note]));
    return paths.value.flatMap((path) => {
      const note = summaries.get(path);
      return note ? [{ path, title: note.title }] : [];
    });
  });

  /** 先迁移旧标签再剪枝，等待同一 tick 的摘要、激活路径和改名事件全部发布。 */
  function reconcile(): void {
    const existing = new Set(options.notes.value.map((note) => note.path));
    const renames = changes;
    changes = [];
    // 新激活路径可能已是改名后的路径，不可再次映射到嵌套目录。
    const migrated = paths.value.map((path) => remap(path, renames));
    const additions = opened.map((path) => existing.has(path) ? path : remap(path, renames));
    opened = [];
    paths.value = options.vaultPath.value
      ? [...new Set([...migrated, ...additions])].filter((path) => existing.has(path))
      : [];
  }

  /** 顺序应用同一轮发布的多次迁移，复用领域层的目录边界规则。 */
  function remap(path: string, renames: NotePathChange[]): string {
    return renames.reduce((current, change) => remapNotePath(current, change.oldPath, change.newPath), path);
  }

  /** 同步捕获每次成功激活，避免同一 tick 连续打开只留下最后一篇。 */
  const stopActive = watch(options.activePath, (path) => {
    ++focusRevision;
    if (path && options.vaultPath.value) opened.push(path);
  }, { flush: "sync", immediate: true });

  /** 只记录迁移，不在同步摘要更新中剪枝，否则旧位置会提前丢失。 */
  const stopChange = watch(options.pathChange, (change) => {
    ++identityRevision;
    if (change) changes.push({ ...change });
  }, { flush: "sync", deep: true });

  /** 路径集合变化立即使在途关闭失效，普通保存更新 mtime 不应取消关闭。 */
  const stopNotes = watch(() => options.notes.value.map((note) => note.path).join("\0"), () => {
    ++identityRevision;
  }, { flush: "sync" });

  /** 换库立即隔离旧标签和旧请求；不把未重新激活的同名路径带入新库。 */
  const stopVault = watch(options.vaultPath, () => {
    ++identityRevision;
    paths.value = [];
    opened = [];
    changes = [];
  }, { flush: "sync" });

  const stopReconcile = watch([options.activePath, options.pathChange,
    () => options.notes.value.map((note) => note.path)], reconcile, { flush: "post", deep: true });
  reconcile();

  /** 保存完成且身份仍有效才关闭；选择失败或用户已离开时绝不强抢焦点。 */
  async function close(path: string): Promise<void> {
    if (closing.value || options.busy.value || disposed || !options.vaultPath.value) return;
    closing.value = true;
    const identity = identityRevision;
    const focus = focusRevision;
    const wasActive = options.activePath.value === path;
    let failure = "笔记保存失败，标签未关闭，请重试。";
    /** 版本守卫还覆盖换库后切回、删除后重建以及卸载后的迟到结果。 */
    function valid(): boolean {
      return !disposed && identity === identityRevision && !options.busy.value && paths.value.includes(path);
    }
    try {
      await nextTick();
      if (!valid()) return;
      await options.flush(path);
      await nextTick();
      if (!valid()) return;
      if (options.activePath.value === path) {
        // 等待期间激活过别处又回来，或后台标签被用户打开，都应保留当前焦点。
        if (!wasActive || focus !== focusRevision) return;
        const index = paths.value.indexOf(path);
        const neighbor = paths.value[index + 1] ?? paths.value[index - 1];
        failure = "无法切换笔记，标签未关闭，请重试。";
        if (neighbor) {
          await options.select(neighbor);
          await nextTick();
          if (!valid()) return;
          // select 可能吞掉错误，必须看到唯一一次成功激活才能移除当前标签。
          if (options.activePath.value !== neighbor || focusRevision !== focus + 1) {
            if (focusRevision === focus) options.onError(failure);
            return;
          }
        } else {
          options.clearActive();
          if (!valid() || options.activePath.value !== null) return;
        }
      }
      paths.value = paths.value.filter((entry) => entry !== path);
    } catch {
      if (valid()) options.onError(failure);
    } finally {
      closing.value = false;
    }
  }

  /** 作用域结束后停止观察并废弃异步结果，避免卸载页面继续选择笔记。 */
  function dispose(): void {
    disposed = true;
    stopActive(); stopChange(); stopNotes(); stopVault(); stopReconcile();
  }
  if (getCurrentScope()) onScopeDispose(dispose);
  return { tabs, close, closing };
}
