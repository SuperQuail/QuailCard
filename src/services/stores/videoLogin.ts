import * as api from "../../api/video";
import type { VideoLoginStatus } from "../../domain/video";
import { resolveError } from "../../utils/errorMessage";

interface LoginState {
  login: VideoLoginStatus | null;
  loginOpen: boolean;
  error: string;
}

/** 独立管理登录命令与会话代次，不反向依赖视频 store。 */
export function createVideoLogin(state: LoginState, onConfirmed: () => Promise<void>) {
  let generation = 0;
  let timer: ReturnType<typeof setTimeout> | undefined;
  let queue: Promise<unknown> = Promise.resolve();

  /** 后端登录共享同一会话，重试和取消也必须等待在途校验完成。 */
  function serialize<T>(operation: () => Promise<T>): Promise<T> {
    const result = queue.then(operation);
    // 失败不能阻塞后续重试；具体错误交给调用者显示。
    queue = result.catch(() => undefined);
    return result;
  }

  /** 先使旧请求失效，再取消尚未发出的定时轮询。 */
  function invalidate(): number {
    generation += 1;
    if (timer !== undefined) clearTimeout(timer);
    timer = undefined;
    return generation;
  }

  /** 失败状态同时驱动弹窗与工作区，清空二维码以停止加载动画。 */
  function fail(error: unknown): void {
    const message = resolveError(error);
    state.error = message;
    state.login = { state: "failed", message, image: "", loggedIn: false };
  }

  /** 只有当前弹窗能接收结果，关闭和重新获取都立即撤销旧请求的写权限。 */
  function current(id: number): boolean {
    return id === generation && state.loginOpen;
  }

  /** 请求完成后才安排下一次轮询，慢速 nav 校验不会积压定时请求。 */
  function schedule(id: number): void {
    // 定时回调只负责启动受代次保护的请求。
    timer = setTimeout(() => { timer = undefined; void request(id, false); }, 1500);
  }

  /** 起始与轮询共享终态处理，所有异步结果（含异常）都检查所属代次。 */
  async function request(id: number, start: boolean): Promise<void> {
    try {
      // 排队期间可能已经关闭或重试，不再发送失效的命令。
      const status = await serialize(async () => {
        if (!current(id)) return undefined;
        return start ? api.loginStart() : api.loginStatus();
      });
      if (!status || !current(id)) return;
      state.login = status;
      if (status.state === "confirmed" || status.loggedIn) {
        state.loginOpen = false;
        await onConfirmed();
      } else if (!["expired", "timeout", "failed"].includes(status.state)) {
        schedule(id);
      }
    } catch (error) {
      if (current(id)) fail(error);
    }
  }

  /** 立即展示新的加载态，同时使旧启动请求和旧轮询结果失效。 */
  async function open(): Promise<void> {
    if (state.login?.loggedIn) return;
    const id = invalidate();
    state.loginOpen = true;
    state.error = "";
    state.login = null;
    await request(id, true);
  }

  /** 立即关闭视图；取消命令按序执行，避免取消后来重新打开的后端会话。 */
  async function close(): Promise<void> {
    const id = invalidate();
    state.loginOpen = false;
    try {
      await serialize(api.loginCancel);
    } catch (error) {
      if (id === generation) state.error = resolveError(error);
    }
  }

  /** 工作区补查也加入串行队列；弹窗已有轮询时不重复查询或覆盖其状态。 */
  async function checkStatus(): Promise<void> {
    const id = generation;
    try {
      // 启动弹窗后旧工作区查询没有继续执行的必要。
      const status = await serialize(async () => {
        if (id === generation && !state.loginOpen) return api.loginStatus();
        return undefined;
      });
      if (status && id === generation && !state.loginOpen) {
        const changed = state.login?.loggedIn !== status.loggedIn;
        state.login = status;
        if (changed) await onConfirmed();
      }
    } catch (error) {
      if (id === generation) fail(error);
    }
  }

  /** 退出与校验共享队列；立即撤销旧请求，只有退出成功才清除凭据状态。 */
  async function logout(): Promise<void> {
    const id = invalidate();
    state.loginOpen = false;
    try {
      await serialize(api.logout);
      if (id !== generation) return;
      state.login = { state: "idle", message: "已退出登录", image: "", loggedIn: false };
      await onConfirmed();
    } catch (error) {
      if (id === generation) state.error = resolveError(error);
    }
  }

  return { open, close, checkStatus, logout };
}
