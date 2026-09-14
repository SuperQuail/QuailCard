import { reactive, watch } from "vue";
import * as api from "../../api/agent";
import type { AgentChange, AgentChildDetailState, AgentChildInfo, AgentPendingWrite, AgentRun, AgentSession, AgentImage } from "../../domain/agent";
import { readAgentImages } from "../agentImages";
import { activeProviderId } from "./providerStore";
import { noteOperationBusy, notePersistence } from "./noteStore";
import { coordinateAgentWrite, undoAgentChange } from "../agentFiles";
import { resolveError } from "../../utils/errorMessage";
import { clearAgentReviews, agentReviewsBusy, settleAgentReviews } from "../agentReview";
import { activeNotePath } from "./noteStore";
import { loadActiveCards, reloadActiveCards } from "./cardStore";
import { refreshStats } from "./reviewStore";
import { createAgentChildren } from "./agentChildren";
import { createAgentChildDetail } from "./agentChildDetail";

/** Agent 独立域状态，组件只接收展示所需的切片。 */
export const agentState = reactive({
  open: false, loading: false, error: "", draft: "", selectedPaths: [] as string[],
  sessions: [] as AgentSession[], session: null as AgentSession | null, run: null as AgentRun | null,
  memory: "", change: null as AgentChange | null, sending: false,
  images: [] as AgentImage[], readingImages: false,
  children: [] as AgentChildInfo[], childrenError: "",
  childDetail: null as AgentChildDetailState | null,
});
let polling: Promise<void> | null = null;
let starting: Promise<AgentRun> | null = null;
let cancelPendingSend = false;
let stopping: Promise<void> | null = null, stoppingGeneration = -1, stoppingOperation = -1;
let sendOperation = 0;
let generation = 0;
const drafts = new Map<string, string>();
const imageDrafts = new Map<string, AgentImage[]>();
export const agentChildren = createAgentChildren(agentState, sendAgentHumanInstruction);
export const agentChildDetail = createAgentChildDetail(agentState);
// 身份和生命周期变化才重建子观察；文字序号变化不触发额外请求。
watch([() => agentState.open, () => agentState.session?.id, () => agentState.run?.id, () => agentState.run?.state], agentChildDetail.sync, { flush: "sync" });

/** 编码期间禁止发送与切会话，避免图片落到别的草稿或迟到发送。 */
export async function pasteAgentImages(files: File[]): Promise<void> {
  if (agentState.readingImages || agentState.loading || agentState.sending) return;
  agentState.readingImages = true; agentState.error = "";
  const epoch = generation;
  try {
    const images = await readAgentImages(files, agentState.images);
    if (epoch === generation) agentState.images.push(...images);
  } catch (error) { if (epoch === generation) agentState.error = resolveError(error); }
  finally { if (epoch === generation) agentState.readingImages = false; }
}

/** 附件移除只影响未发送草稿，不改写历史消息。 */
export function removeAgentImage(index: number): void {
  if (!agentState.readingImages && !agentState.sending) agentState.images.splice(index, 1);
}

/** 可见入口初始化模型和会话，保留已有输入与运行状态。 */
export async function openAgent(): Promise<void> {
  agentState.open = true;
  if (agentState.session || agentState.loading) return;
  agentState.loading = true;
  const epoch = generation;
  try {
    const [sessions, memory] = await Promise.all([api.sessions(), api.memory()]);
    if (epoch !== generation) return;
    const session = sessions.length ? await api.session(sessions[0].id) : await api.createSession();
    if (epoch !== generation) return;
    agentState.sessions = sessions.some(item => item.id === session.id) ? sessions : [session, ...sessions]; agentState.memory = memory.content; agentState.session = session;
    agentState.selectedPaths = [...session.selectedPaths];
  } catch (error) { if (epoch === generation) agentState.error = resolveError(error); }
  finally { if (epoch === generation) { agentState.loading = false; void agentChildren.refreshChildren(); } }
}

/** 会话切换保留各自草稿；运行任务结束前不改变当前对话身份。 */
export async function selectAgentSession(id?: string): Promise<void> {
  if (agentState.readingImages) return;
  if (agentState.sending || agentState.run?.state === "running" || agentState.loading) return;
  agentState.loading = true;
  const epoch = ++generation; agentChildren.reset();
  if (agentState.session) drafts.set(agentState.session.id, agentState.draft);
  if (agentState.session) imageDrafts.set(agentState.session.id, [...agentState.images]);
  try {
    const session = id ? await api.session(id) : await api.createSession();
    if (epoch !== generation) return;
    agentState.session = session;
    agentState.draft = drafts.get(agentState.session.id) ?? "";
    agentState.images = [...(imageDrafts.get(session.id) ?? [])];
    agentState.selectedPaths = [...agentState.session.selectedPaths];
    agentState.run = null; agentState.error = "";
    const sessions = await api.sessions();
    if (epoch === generation) agentState.sessions = sessions;
  } catch (error) { if (epoch === generation) agentState.error = resolveError(error); }
  finally { if (epoch === generation) { agentState.loading = false; void agentChildren.refreshChildren(); } }
}

/** 后端删除成功才更新列表，删除当前会话后恢复最近对话的草稿。 */
export async function deleteAgentSession(id: string): Promise<void> {
  if (agentState.readingImages) return;
  if (agentState.loading || agentState.sending || agentState.run?.state === "running") return;
  if (agentReviewsBusy()) { agentState.error = "请等待当前复习提交完成后再删除会话"; return; }
  agentState.loading = true;
  agentState.error = "";
  const epoch = ++generation;
  if (agentState.session?.id === id) agentChildren.reset();
  try {
    await api.deleteSession(id);
    if (epoch !== generation) return;
    drafts.delete(id);
    imageDrafts.delete(id);
    agentState.sessions = agentState.sessions.filter(item => item.id !== id);
    if (agentState.session?.id === id) {
      agentState.session = null;
      agentState.run = null;
      agentState.draft = "";
      agentState.images = [];
      agentState.selectedPaths = [];
      clearAgentReviews();
      const next = agentState.sessions[0];
      if (next) {
        const session = await api.session(next.id);
        if (epoch !== generation) return;
        agentState.session = session;
        agentState.draft = drafts.get(session.id) ?? "";
        agentState.images = [...(imageDrafts.get(session.id) ?? [])];
        agentState.selectedPaths = [...session.selectedPaths];
      }
    }
  } catch (error) { if (epoch === generation) agentState.error = resolveError(error); }
  finally { if (epoch === generation) { agentState.loading = false; void agentChildren.refreshChildren(); } }
}

/** 普通聊天显式发送当前附件，成功登记后才清空该会话草稿。 */
export async function sendAgentMessage(content = agentState.draft): Promise<void> {
  await sendAgentInput(content);
}

/** 管理操作不读取、不替换草稿和附件；目标身份必须由点击时的 root 固定。 */
async function sendAgentHumanInstruction(content: string, sessionId: string): Promise<void> {
  await sendAgentInput(content, sessionId);
}

/** 共享任务登记与轮询；固定输入身份并拒绝迟到或跨会话的返回值。 */
async function sendAgentInput(content: string, expectedSessionId?: string): Promise<void> {
  const management = expectedSessionId !== undefined;
  if (management && (agentState.session?.id !== expectedSessionId || agentState.session?.parentSessionId)) return;
  if ((!content.trim() && (management || !agentState.images.length)) || agentState.readingImages || agentState.loading || agentState.sending || agentState.run?.state === "running") return;
  if (agentReviewsBusy()) { agentState.error = "请等待当前复习提交完成后再发送消息"; return; }
  sendOperation += 1; cancelPendingSend = false;
  agentState.sending = true; agentState.error = "";
  const epoch = generation;
  try {
    if (!agentState.session) await openAgent();
    if (epoch !== generation || !agentState.session) return;
    const sessionId = agentState.session.id;
    if ((management && sessionId !== expectedSessionId) || (agentState.run && agentState.run.sessionId !== sessionId)) return;
    const images = management ? [] : [...agentState.images];
    const providerId = activeProviderId.value, selectedPaths = [...agentState.selectedPaths];
    await notePersistence.flushAll();
    if (cancelPendingSend || epoch !== generation || agentState.session?.id !== sessionId) return;
    const requestId = crypto.randomUUID();
    starting = api.send({ sessionId, requestId, content, images, providerId, selectedPaths }).catch(async error => {
      try { return await api.status(requestId); } catch { throw error; }
    });
    const run = await starting;
    if (epoch !== generation || agentState.session?.id !== sessionId) return;
    starting = null;
    if (run.sessionId !== sessionId) throw new Error("发送结果与当前会话不匹配，请重试。");
    agentState.run = run;
    if (!management) {
      agentState.draft = ""; agentState.images = [];
      drafts.delete(sessionId); imageDrafts.delete(sessionId);
    }
    // 活动任务直接建立唯一观察轮询，避免加载历史期间停止另起一条 status 循环。
    if (run.state !== "running") try {
      const session = await api.session(sessionId);
      if (epoch === generation && agentState.session?.id === sessionId && session.id === sessionId) agentState.session = session;
    } catch (error) { if (epoch === generation) agentState.error = management ? "无法刷新当前会话，请重试。" : resolveError(error); }
    if (epoch !== generation || agentState.session?.id !== sessionId) return;
    polling = poll(epoch); await polling;
  } catch (error) {
    if (epoch === generation) { if (management) throw new Error("管理请求失败，请重试。"); agentState.error = resolveError(error); }
  } finally { if (epoch === generation) { agentState.sending = false; polling = null; starting = null; } }
}

/** 每轮只读取版本化观察；正文版本不变时保留历史对象，流式状态仍实时推进。 */
async function poll(epoch: number): Promise<void> {
  let failures = 0, revision: string | null = null;
  const sessionId = agentState.session?.id;
  if (!sessionId) return;
  while (epoch === generation && agentState.run?.state === "running") {
    const id = agentState.run.id;
    // null 表示观察失败、整树写入列表未知，此时才退回根快照。
    let run: AgentRun | null = null, writes: AgentPendingWrite[] | null = null;
    try {
      const observation = await api.observe(sessionId, id, null, revision);
      if (epoch !== generation || agentState.session?.id !== sessionId) return;
      run = observation.run;
      if (observation.sessionId !== sessionId || !run || run.sessionId !== sessionId || run.id !== id
        || (observation.session && observation.session.id !== sessionId)
        || !observation.revision || (!observation.session && observation.revision !== revision)) {
        throw new Error("运行观察与当前会话不匹配，请重试。");
      }
      if (observation.session) agentState.session = observation.session;
      writes = observation.writes;
      revision = observation.revision; failures = 0;
    } catch (error) {
      if (epoch !== generation || agentState.session?.id !== sessionId) return;
      failures += 1; agentState.error = resolveError(error);
      // 历史读盘失败也必须能观察停止终态，不能让取消握手无限等待损坏记录。
      run = await api.status(id).catch(() => null);
      // 整树写入列表来自独立只读命令，历史损坏不能变成子代理的保存死等。
      writes = await api.pendingWrites(id).catch(() => null);
    }
    if (epoch !== generation || agentState.session?.id !== sessionId) return;
    const active = run && run.sessionId === sessionId && run.id === id ? run : null;
    if (active && active.sequence >= (agentState.run?.sequence ?? 0)) agentState.run = active;
    // 观察成功以后端整树列表为准；只在观察失败时用根快照兜底。
    const pending = active?.state === "running" ? (writes ?? rootPendingWrite(active)) : [];
    if (epoch !== generation || agentState.session?.id !== sessionId) return;
    if (pending.length) await coordinatePendingWrites(epoch, sessionId, id, pending);
    if (epoch !== generation || agentState.session?.id !== sessionId) return;
    void agentChildren.refreshChildren(false);
    if (failures >= 5) agentState.error = "连接暂时中断，正在尝试恢复；可点击停止。";
    if (agentState.run?.state === "running") await new Promise(resolve => setTimeout(resolve, failures ? 1500 : 250));
  }
  if (epoch === generation) {
    const sessions = await api.sessions();
    if (epoch !== generation || agentState.session?.id !== sessionId) return;
    agentState.sessions = sessions;
    void agentChildren.refreshChildren();
    if (agentState.run?.error) agentState.error = agentState.run.error;
    await refreshAfterCardDeletion();
  }
}

/** 根快照自身也能给出保存等待，供历史读取失败时兜底。 */
function rootPendingWrite(run: AgentRun): AgentPendingWrite[] {
  return run.pendingWrite && run.pendingWriteId
    ? [{ executionId: run.id, sessionId: run.sessionId, path: run.pendingWrite, operationId: run.pendingWriteId }]
    : [];
}

/** 整树写入协调：根与子代理共用同一条编辑器只读窗口，串行保存后才确认写入。 */
async function coordinatePendingWrites(epoch: number, sessionId: string, runId: string, writes: AgentPendingWrite[]): Promise<void> {
  for (const write of writes) {
    if (epoch !== generation || agentState.session?.id !== sessionId || agentState.run?.id !== runId) return;
    // 编辑器正忙或保存失败都留到下一轮重试，不取消仍在等待保存的执行。
    if (noteOperationBusy.value) return;
    try {
      await coordinateAgentWrite(() => api.acknowledgeWrite(runId, write.executionId, write.operationId));
    } catch (error) { if (epoch === generation) agentState.error = resolveError(error); return; }
  }
}

/** Agent 删除卡片后刷新当前笔记面板与统计；没有删除回合时不动卡片状态。 */
async function refreshAfterCardDeletion(): Promise<void> {
  const deleted = (agentState.session?.messages ?? []).some(message => message.kind === "card" && message.data?.state === "deleted");
  if (!deleted) return;
  try { await reloadActiveCards(); await refreshStats(); }
  catch (error) { agentState.error = resolveError(error); }
}

/** 停止意图覆盖发送准备阶段；重复点击共享握手，迟到状态不能跨库写回。 */
export function stopAgent(): Promise<void> {
  cancelPendingSend = true;
  const epoch = generation, sessionId = agentState.session?.id;
  if (stopping && stoppingGeneration === epoch && stoppingOperation === sendOperation) return stopping;
  const pendingStart = starting, initialRun = agentState.run;
  stoppingGeneration = epoch; stoppingOperation = sendOperation;
  const work = Promise.resolve().then(async () => {
    const run = pendingStart ? await pendingStart : initialRun;
    if (!run || run.sessionId !== sessionId) return;
    const id = run.id;
    /** 只允许当前库、会话与执行接收状态，不能以迟到快照复活旧任务。 */
    const current = () => epoch === generation && agentState.session?.id === sessionId && agentState.run?.id === id;
    if (!current() || agentState.run?.state !== "running") return;
    const activePoll = polling;
    await api.cancel(id);
    if (!current()) return;
    if (activePoll) { await activePoll; return; }
    while (current() && agentState.run?.state === "running") {
      const next = await api.status(id);
      if (!current()) return;
      if (next.id !== id || next.sessionId !== sessionId) throw new Error("停止状态与当前任务不匹配，请重试。");
      if (next.sequence >= (agentState.run?.sequence ?? 0)) agentState.run = next;
      if (agentState.run?.state === "running") await new Promise(resolve => setTimeout(resolve, 100));
    }
  });
  const result = work.finally(() => { if (stopping === result) stopping = null; });
  stopping = result;
  return result;
}

/** 切库前停止并清空所有当前库状态，避免记忆或历史跨库串用。 */
export async function leaveAgentVault(): Promise<void> {
  agentState.open = false;
  await stopAgent(); await settleAgentReviews(); generation += 1; agentChildren.reset(); drafts.clear(); imageDrafts.clear(); clearAgentReviews();
  agentState.images = []; agentState.readingImages = false;
  Object.assign(agentState, { open: false, loading: false, sending: false, session: null, sessions: [], run: null, draft: "", selectedPaths: [], memory: "", change: null, error: "" });
}

/** 差异内容按需读取，避免把全部恢复正文常驻前端状态。 */
export async function showAgentChange(id: string): Promise<void> {
  try { agentState.change = await api.change(id); }
  catch (error) { agentState.error = resolveError(error); }
}
/** 只在后端确认撤销后更新展示状态。 */
export async function undoAgent(id: string): Promise<void> {
  try { await undoAgentChange(id); agentState.change = await api.change(id); }
  catch (error) { agentState.error = resolveError(error); }
}
/** 记忆仅通过用户点击保存写入。 */
export async function saveAgentMemory(content: string): Promise<void> {
  try { agentState.memory = (await api.saveMemory(content)).content; }
  catch (error) { agentState.error = resolveError(error); }
}

/** 采纳固定会话中的草稿，重试使用相同身份避免重复卡片。 */
export async function adoptAgentCards(messageId: string, ids: string[]): Promise<void> {
  if (!agentState.session || agentState.loading || agentState.sending || agentState.run?.state === "running") return;
  agentState.loading = true;
  const sessionId = agentState.session.id;
  try {
    await coordinateAgentWrite(async () => { agentState.session = await api.adoptCards(sessionId, messageId, ids); });
    if (activeNotePath.value) await loadActiveCards(activeNotePath.value);
  } catch (error) { agentState.error = resolveError(error); }
  finally { agentState.loading = false; }
}
