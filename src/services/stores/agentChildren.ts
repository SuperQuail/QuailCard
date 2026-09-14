import * as api from "../../api/agent";
import type { AgentChildInfo, AgentRun, AgentSession } from "../../domain/agent";

/** 子树状态不持有聊天草稿，管理消息只能通过显式人类发送通道。 */
export interface AgentChildrenState {
  session: AgentSession | null; run: AgentRun | null; loading: boolean; sending: boolean;
  children: AgentChildInfo[]; childrenError: string;
}

/** 摘要未变化时复用数组与行对象，避免状态轮询让已展开的树反复重建。 */
function sameChildren(before: AgentChildInfo[], after: AgentChildInfo[]): boolean {
  return before.length === after.length && before.every((child, index) => {
    const next = after[index], scope = child.writeScope ?? [], nextScope = next.writeScope ?? [];
    return child.agentId === next.agentId && child.parentSessionId === next.parentSessionId
      && child.description === next.description && child.status === next.status && child.delegationDepth === next.delegationDepth
      && scope.length === nextScope.length && scope.every((path, i) => path === nextScope[i]);
  });
}

/** 与 root 生命周期同生共灭；epoch 使同一 root 离开再返回也不会接纳旧响应。 */
export function createAgentChildren(
  state: AgentChildrenState,
  sendHuman: (content: string, sessionId: string) => Promise<void>,
) {
  let epoch = 0, lastRefresh = -Infinity;
  let listPending: Promise<void> | null = null, queued = false;

  /** 离开 root 立即废弃请求；后端任务不因离开展示而取消。 */
  function reset(): void {
    epoch += 1; lastRefresh = -Infinity;
    // 旧请求仍在途时保留物理槽，新根仅排队一次后继读取。
    queued = false;
    state.children = []; state.childrenError = "";
  }
  /** 异步提交必须同时匹配 epoch 与当前 root，而非仅比较 run 身份。 */
  function current(token: number, sessionId: string): boolean {
    return token === epoch && state.session?.id === sessionId;
  }
  /** 禁止对已切换或子会话使用根管理入口；后端仍负责权威父链检查。 */
  function root(): AgentSession | null {
    const session = state.session;
    if (!session || session.parentSessionId || state.loading || (state.run && state.run.sessionId !== session.id)) return null;
    return session;
  }
  /** root poll 每 1.5 秒至多一次；跨根切换也保持唯一物理请求。 */
  function refreshChildren(force = true): Promise<void> {
    if (!root()) return Promise.resolve();
    if (listPending) { if (force) queued = true; return listPending; }
    if (!force && Date.now() - lastRefresh < 1500) return Promise.resolve();
    queued = true;
    listPending = Promise.resolve().then(async () => {
      while (queued) {
        queued = false;
        const session = root(), token = epoch;
        if (!session) continue;
        const runId = state.run?.state === "running" ? state.run.id : null;
        lastRefresh = Date.now();
        try {
          const children = await api.children(session.id, runId);
          if (!current(token, session.id)) continue;
          if (runId !== (state.run?.state === "running" ? state.run.id : null)) queued = true;
          if (!queued) {
            if (!sameChildren(state.children, children)) state.children = children;
            state.childrenError = "";
          }
        } catch {
          if (current(token, session.id) && !queued) state.childrenError = "无法刷新子代理，请重试。";
        }
      }
    }).finally(() => {
      listPending = null;
      if (queued) void refreshChildren();
    });
    return listPending;
  }
  /** 仅中断当前活动树中正在工作的子代理，不用旧 run 操作新 root。 */
  async function interruptChild(id: string): Promise<void> {
    const session = root(), run = state.run;
    if (!session || run?.state !== "running" || !state.children.some(child => child.agentId === id && child.status === "running")) {
      state.childrenError = "该子代理当前不可中断，请刷新列表。"; return;
    }
    const token = epoch; state.childrenError = "";
    try {
      await api.interruptChild(run.id, id);
      if (current(token, session.id)) await refreshChildren();
    } catch { if (current(token, session.id)) state.childrenError = "中断子代理失败，请刷新后重试。"; }
  }
  /** 活动树直发；root 闲置时明确以人类消息启动，由模型 send_message 冷恢复。 */
  async function messageChild(id: string, message: string): Promise<void> {
    const session = root(), child = state.children.find(item => item.agentId === id);
    if (!session || !child || child.parentSessionId !== session.id || !message.trim()) {
      state.childrenError = "只能向当前会话的直接子代理发送非空追问。"; return;
    }
    if (new TextEncoder().encode(message).byteLength > 16 * 1024) {
      state.childrenError = "追问内容不能超过 16 KiB，请缩短后重试。"; return;
    }
    const token = epoch; state.childrenError = "";
    try {
      if (state.run?.state === "running") await api.messageChild(state.run.id, id, message);
      else {
        if (state.sending) return;
        await sendHuman(
          "请继续当前主会话。我要求你使用 send_message 向直接子代理 " + JSON.stringify(id) + " 发送以下追问；如其未加载，请冷恢复该子代理。追问内容：\n" + message,
          session.id,
        );
      }
      if (current(token, session.id)) await refreshChildren();
    } catch { if (current(token, session.id)) state.childrenError = "发送子代理追问失败，请重试。"; }
  }
  /** 恢复必须是用户明确的继续请求，前端不自行修改持久化 Goal 或伪造后台许可。 */
  async function resumeGoal(): Promise<void> {
    const session = root();
    if (!session?.goal || session.goal.phase === "complete" || state.run?.state === "running" || state.sending) return;
    const token = epoch; state.childrenError = "";
    try { await sendHuman("请继续当前会话的目标，恢复执行并按已有计划推进；若仍有阻塞，请明确说明。", session.id); }
    catch { if (current(token, session.id)) state.childrenError = "恢复目标失败，请重试。"; }
  }
  return { reset, refreshChildren, interruptChild, messageChild, resumeGoal };
}
