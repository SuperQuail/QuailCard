import * as api from "../../api/agent";
import type { AgentChildDetailState, AgentRun, AgentSession } from "../../domain/agent";

/** 只读取根身份与可见性；子详情不能改动父任务、输入草稿或执行许可。 */
export interface AgentChildDetailHost {
  open: boolean; session: AgentSession | null; run: AgentRun | null;
  childDetail: AgentChildDetailState | null;
}

/** 每个工作区只有一个物理观察请求；快速切换只保留最新目标，不堆积旧 IPC。 */
export function createAgentChildDetail(state: AgentChildDetailHost) {
  let epoch = 0, owner = "", revision: string | null = null, queued = false;
  let timer: ReturnType<typeof setTimeout> | undefined;
  let pending: Promise<void> | null = null;

  /** 只有当前根正在运行时才附带运行身份，历史查询不会启动网络执行。 */
  function runId(rootId = owner): string | null {
    return state.run?.sessionId === rootId && state.run.state === "running" ? state.run.id : null;
  }
  /** 每次调度前清除旧计时器，自动刷新与手动刷新不能叠加。 */
  function clearTimer(): void { clearTimeout(timer); timer = undefined; }
  /** 关闭废弃观察意图，但保留仍在途的 transport，直到其完成才能读取新目标。 */
  function closeChild(): void {
    epoch += 1; clearTimer(); queued = false; revision = null; owner = "";
    state.childDetail = null;
  }
  /** 响应同时绑定可见详情、根和执行代次，旧响应无权推进新详情的版本。 */
  function current(token: number, rootId: string, detail: AgentChildDetailState, observedRun: string | null): boolean {
    return token === epoch && state.open && state.session?.id === rootId && owner === rootId
      && state.childDetail === detail && runId(rootId) === observedRun;
  }
  /** 根仍活跃才定时观察，根结束后的最后刷新由宿主同步入口触发。 */
  function schedule(): void {
    clearTimer();
    if (state.open && state.childDetail && runId()) {
      timer = setTimeout(() => { timer = undefined; void refreshChild(); }, state.childDetail.error ? 1500 : 750);
    }
  }
  /** 每次实际调用前捕获最新目标；关闭发生在微任务前时不发空身份请求。 */
  async function readLatest(): Promise<void> {
    const detail = state.childDetail, rootId = owner;
    if (!detail || !state.open || state.session?.id !== rootId) return;
    const token = epoch, observedRun = runId(rootId), known = revision;
    detail.loading = !detail.session;
    try {
      const observation = await api.observe(rootId, observedRun, detail.id, known);
      if (!current(token, rootId, detail, observedRun)) return;
      if (observation.sessionId !== detail.id || (observation.session && observation.session.id !== detail.id)
        || (observation.run && observation.run.sessionId !== detail.id)
        || !observation.revision || (!observation.session && (!detail.session || known !== observation.revision))) {
        throw new Error("子会话观察身份不匹配");
      }
      if (observation.session) detail.session = observation.session;
      revision = observation.revision;
      const next = observation.run, previous = detail.run;
      if (!next || !previous || next.id !== previous.id || next.sequence >= previous.sequence) detail.run = next;
      detail.error = "";
    } catch {
      if (current(token, rootId, detail, observedRun)) detail.error = "无法读取子代理详情，请重试。";
    } finally {
      if (current(token, rootId, detail, observedRun)) detail.loading = false;
    }
  }
  /** 同目标重复刷新复用进行中请求，目标变化时只排队最新一次读取。 */
  function refreshChild(): Promise<void> {
    if (!state.childDetail || !state.open || state.session?.id !== owner) return Promise.resolve();
    clearTimer();
    if (pending) return pending;
    queued = true;
    pending = Promise.resolve().then(async () => {
      while (queued) { queued = false; await readLatest(); }
    }).finally(() => {
      pending = null;
      if (queued) void refreshChild();
      else schedule();
    });
    return pending;
  }
  /** 查看是独立只读动作，不等待父任务完成；父链授权最终由后端核验。 */
  function openChild(id: string): Promise<void> {
    const root = state.session;
    if (!state.open || !root || root.parentSessionId || !id || id === root.id) return Promise.resolve();
    if (state.childDetail?.id === id && owner === root.id) return refreshChild();
    closeChild(); owner = root.id;
    state.childDetail = { id, session: null, run: null, loading: true, error: "" };
    queued = true;
    return refreshChild();
  }
  /** 生命周期变化只更新观察意图，不清除物理请求，避免快速换 run 堆积读盘。 */
  function sync(): void {
    if (!state.childDetail) return;
    if (!state.open || state.session?.id !== owner) { closeChild(); return; }
    epoch += 1; clearTimer(); queued = true;
    state.childDetail.run = null;
    void refreshChild();
  }
  return { openChild, closeChild, refreshChild, sync };
}
