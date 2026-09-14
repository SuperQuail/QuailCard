import { reactive } from "vue";
import { resolveError } from "../utils/errorMessage";

export interface NoteSaveState {
  content: string;
  savedContent: string;
  status: "saved" | "dirty" | "saving" | "error";
  error: string;
  lineEnding: "\n" | "\r\n";
  revision: number;
}

/** 每篇笔记保留草稿，所有写入串行化；只有实际写盘成功才发布保存状态。 */
export function createNotePersistence(
  write: (path: string, content: string) => Promise<number>,
  onSaved: (path: string, content: string, mtime: number) => void,
) {
  const states = reactive(new Map<string, NoteSaveState>());
  const timers = new Map<string, ReturnType<typeof setTimeout>>();
  let tail: Promise<unknown> = Promise.resolve();

  /** 打开笔记时登记磁盘内容；未保存草稿优先，不能被失焦扫描覆盖。 */
  function register(path: string, content: string, readRevision?: number): string {
    const existing = states.get(path);
    if (existing && (existing.status !== "saved" || (readRevision !== undefined && readRevision !== existing.revision))) return existing.content;
    const lineEnding = content.includes("\r\n") ? "\r\n" : "\n";
    const normalized = content.replace(/\r\n/g, "\n");
    states.set(path, { content: normalized, savedContent: normalized, status: "saved", error: "", lineEnding, revision: existing?.revision ?? 0 });
    return normalized;
  }

  /** 清除防抖计时器，避免改名后仍有旧路径任务。 */
  function cancelTimer(path: string): void {
    const timer = timers.get(path);
    if (timer) clearTimeout(timer);
    timers.delete(path);
  }

  /** 即时收下草稿并防抖保存；后台失败保存在状态中供重试。 */
  function update(path: string, content: string): void {
    if (!states.has(path)) register(path, "");
    const state = states.get(path)!;
    state.content = content.replace(/\r\n/g, "\n");
    state.revision += 1;
    state.error = "";
    if (state.status !== "saving") state.status = content === state.savedContent ? "saved" : "dirty";
    cancelTimer(path);
    if (state.status !== "saved") {
      timers.set(path, setTimeout(() => { void flush(path).catch(() => {}); }, 600));
    }
  }

  /** 排队写入最新草稿；失败保留内容，后续重试不会被 rejected promise 阻断。 */
  function flush(path: string): Promise<void> {
    cancelTimer(path);
    const task = tail.catch(() => {}).then(async () => {
      const state = states.get(path);
      if (!state) return;
      while (state.content !== state.savedContent) {
        const content = state.content;
        state.status = "saving";
        state.error = "";
        try {
          const mtime = await write(path, content.replace(/\n/g, state.lineEnding));
          state.savedContent = content;
          state.revision += 1;
          onSaved(path, content, mtime);
        } catch (error) {
          state.status = "error";
          state.error = resolveError(error);
          throw error;
        }
      }
      state.status = "saved";
      state.error = "";
    });
    tail = task;
    return task;
  }

  /** 改名、换库之前清空全部未完成写入，任一失败都阻止后续操作。 */
  async function flushAll(): Promise<void> {
    for (const path of states.keys()) await flush(path);
  }

  /** 路径操作成功后迁移草稿身份，不能留下旧路径计时器。 */
  function rename(oldPath: string, newPath: string): void {
    for (const [path, state] of [...states]) {
      if (path !== oldPath && !path.startsWith(`${oldPath}/`)) continue;
      cancelTimer(path);
      states.delete(path);
      states.set(`${newPath}${path.slice(oldPath.length)}`, state);
    }
  }

  /** 删除或离开 Vault 后清理已处理的草稿，调用者必须先完成保存协调。 */
  function remove(prefix?: string): void {
    for (const path of [...states.keys()]) {
      if (prefix && path !== prefix && !path.startsWith(`${prefix}/`)) continue;
      cancelTimer(path);
      states.delete(path);
    }
  }

  return { states, register, update, flush, flushAll, rename, remove };
}
