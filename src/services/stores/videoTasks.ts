import * as api from "../../api/video";
import type { VideoStartInput, VideoTaskStatus } from "../../domain/video";
import { resolveError } from "../../utils/errorMessage";

interface TaskState {
  task: VideoTaskStatus | null;
  starting: boolean;
  error: string;
  taskError: string;
}

/** 单任务控制器保存原始输入，并以完成后计时避免网络慢时重叠轮询。 */
export function createVideoTasks(state: TaskState, captureCompletion?: () => (notePath: string) => void | Promise<void>) {
  let generation = 0;
  let timer: ReturnType<typeof setTimeout> | undefined;
  let pending: Promise<void> | undefined;
  let original: VideoStartInput | undefined;

  let onCompleted: ((notePath: string) => void | Promise<void>) | undefined;
  let completionSent = false;

  /** 完成副作用最多一次；刷新失败不改变生成结果，也不重新生成笔记。 */
  async function complete(status: VideoTaskStatus, epoch: number): Promise<void> {
    if (status.state !== "completed" || !status.notePath || completionSent) return;
    completionSent = true;
    try { await onCompleted?.(status.notePath); }
    catch (error) {
      if (epoch === generation && state.task?.taskId === status.taskId) state.taskError = resolveError(error);
    }
  }

  /** 旧任务及倒退序号都不能覆盖当前快照，更不能触发完成副作用。 */
  async function accept(status: VideoTaskStatus, id: string, epoch: number): Promise<void> {
    if (epoch !== generation || state.task?.taskId !== id || status.taskId !== id) return;
    if (status.sequence < state.task.sequence) return;
    if (state.task.state !== "running" && status.state === "running") return;
    state.task = status;
    state.taskError = "";
    await complete(status, epoch);
  }

  /** 在途查询可以被停止操作复用，临时错误保留快照并继续恢复。 */
  function poll(): Promise<void> {
    if (pending) return pending;
    const id = state.task?.taskId;
    const epoch = generation;
    if (!id || state.task?.state !== "running") return Promise.resolve();
    if (timer !== undefined) clearTimeout(timer);
    timer = undefined;
    pending = (async () => {
      try {
        const status = await api.status(id);
        await accept(status, id, epoch);
      } catch (error) {
        if (epoch === generation && state.task?.taskId === id) state.taskError = resolveError(error);
      } finally {
        pending = undefined;
        if (epoch === generation && state.task?.taskId === id && state.task.state === "running") {
          timer = setTimeout(() => { void poll(); }, 1500);
        }
      }
    })();
    return pending;
  }

  /** 重复点击在发送命令前即被拦截，重试始终使用上次提交而非可编辑表单。 */
  async function start(input: VideoStartInput): Promise<void> {
    if (state.starting || state.task?.state === "running") return;
    state.starting = true;
    state.error = "";
    state.taskError = "";
    generation += 1;
    if (timer !== undefined) clearTimeout(timer);
    original = { ...input, pages: [...input.pages] };
    try {
      // 等旧轮询结束后再开始新任务，避免跨任务状态请求重叠。
      await pending;
      // 紧邻实际发起命令时捕获归属，等待旧刷新期间也可能已经换库。
      onCompleted = captureCompletion?.();
      completionSent = false;
      state.task = await api.start({ ...original, pages: [...original.pages] });
      if (state.task.state === "running") timer = setTimeout(() => { void poll(); }, 1500);
      else await complete(state.task, generation);
    } catch (error) { state.error = resolveError(error); }
    finally { state.starting = false; }
  }

  /** 取消只针对捕获的任务；查询仍由同一个串行入口负责。 */
  async function stop(): Promise<void> {
    const id = state.task?.taskId;
    if (!id || state.task?.state !== "running") return;
    try { await api.cancel(id); if (state.task?.taskId === id) await poll(); }
    catch (error) { if (state.task?.taskId === id) state.error = resolveError(error); }
  }

  /** 历史恢复不是重试；没有内存中的原始输入时不猜测参数。 */
  async function retry(): Promise<void> {
    if (original) await start(original);
    else state.error = "没有原始任务参数，请从历史记录恢复后确认再开始";
  }

  return { start, stop, retry, poll };
}
