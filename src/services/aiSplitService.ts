import { ref } from "vue";
import type { AdoptCardsInput, AdoptCardsResult, GenerationInput, GenerationTaskStart, GenerationTaskStatus } from "../domain/types";
import { resolveError } from "../utils/errorMessage";
import { emptyAiSplitState, type AiSplitSnapshot } from "./aiSplitTypes";

/** 用例只依赖所需操作，便于以受控异步响应验证竞态。 */
export interface AiSplitPorts {
  providerReady(): boolean;
  verifySnapshot(snapshot: AiSplitSnapshot): Promise<string>;
  startGeneration(input: GenerationInput): Promise<GenerationTaskStart>;
  getGenerationStatus(taskId: string): Promise<GenerationTaskStatus>;
  cancelGeneration(taskId: string): Promise<GenerationTaskStatus>;
  adoptCards(input: AdoptCardsInput): Promise<AdoptCardsResult>;
  refresh(snapshot: AiSplitSnapshot): Promise<void>;
  showToast(message: string): void;
}

const modes = { vocabulary: "dictation", qa: "ai-review", ai: "ai-review" } as const;

/** 一个用例实例管理一个窗口任务，停止保留草稿，关闭废弃当前会话。 */
export function createAiSplitService(ports: AiSplitPorts) {
  const state = ref(emptyAiSplitState());
  let revision = 0;
  let timer: ReturnType<typeof setTimeout> | undefined;

  /** 终态及关闭时停止查询，避免闲置任务继续占用 IPC。 */
  function clearPoll(): void {
    if (timer !== undefined) clearTimeout(timer);
    timer = undefined;
  }

  /** 只有当前会话仍在运行时才接收阶段与草稿。 */
  function isRunning(session: number): boolean {
    return session === revision && state.value.open && state.value.step === "running";
  }

  /** 终态草稿原样保留全部字段，勾选由稳定 UUID 标识。 */
  function receive(status: GenerationTaskStatus, session: number): void {
    if (!isRunning(session)) return;
    state.value.phase = status.phase;
    state.value.generatedCount = status.generatedCount;
    if (status.state === "running") return;
    clearPoll();
    state.value.stopping = false;
    state.value.drafts = status.result?.cards ?? [];
    state.value.accepted = new Set(state.value.drafts.map((card) => card.draftId));
    state.value.warnings = status.result?.warnings ?? [];
    state.value.step = status.state === "failed" && !state.value.drafts.length ? "scope" : "drafts";
    if (status.error) ports.showToast(status.error.message);
  }

  /** 查询按响应完成后间隔 500ms 发起，避免慢请求互相重叠。 */
  function schedulePoll(taskId: string, session: number): void {
    if (!isRunning(session)) return;
    clearPoll();
    timer = setTimeout(() => { void poll(taskId, session); }, 500);
  }

  /** 临时查询失败继续等待草稿，任务过期等确定性错误结束轮询。 */
  async function poll(taskId: string, session: number): Promise<void> {
    try {
      receive(await ports.getGenerationStatus(taskId), session);
    } catch (error) {
      if (!isRunning(session)) return;
      const message = `读取生成进度失败：${resolveError(error)}`;
      if (!state.value.warnings.includes(message)) state.value.warnings.push(message);
      const code = typeof error === "object" && error !== null && "code" in error ? String(error.code) : "";
      if (["TASK_NOT_FOUND", "TASK_EXPIRED", "GENERATION_TASK_NOT_FOUND", "NOT_FOUND", "FORBIDDEN"].includes(code)) {
        clearPoll();
        state.value.step = state.value.drafts.length ? "drafts" : "scope";
        state.value.stopping = false;
        ports.showToast(message);
        return;
      }
    }
    schedulePoll(taskId, session);
  }

  /** 后端登记完成前关闭，也必须在拿到任务 ID 后补发取消。 */
  async function cancelDetached(taskId: string): Promise<void> {
    try { await ports.cancelGeneration(taskId); }
    catch (error) { ports.showToast(`停止生成失败：${resolveError(error)}`); }
  }

  /** 新会话冻结来源并默认使用有效编辑器选区。 */
  function open(snapshot: AiSplitSnapshot): void {
    if (state.value.open) return;
    ++revision;
    state.value = { ...emptyAiSplitState(), open: true, snapshot, scope: snapshot.selection ? "selection" : "note" };
  }

  /** 关闭使所有迟到响应失效；正在采纳时等待提交结果。 */
  function close(): void {
    if (state.value.saving) return;
    const taskId = state.value.step === "running" ? state.value.taskId : null;
    ++revision;
    clearPoll();
    state.value = emptyAiSplitState();
    if (taskId) void cancelDetached(taskId);
  }

  /** 核对已保存的原笔记后启动，避免异步保存和取消互相覆盖。 */
  async function start(): Promise<void> {
    const current = state.value;
    const snapshot = current.snapshot;
    if (!snapshot || current.step !== "scope" || current.invalidReason || !ports.providerReady()) return;
    const session = ++revision;
    current.step = "running";
    current.phase = "preparing";
    current.warnings = [];
    current.taskId = null;
    current.generatedCount = 0;
    current.stopping = false;
    current.drafts = [];
    current.accepted = new Set();
    try {
      const noteHash = await ports.verifySnapshot(snapshot);
      if (!isRunning(session)) return;
      if (current.stopping) { current.step = "drafts"; current.stopping = false; return; }
      const { taskId } = await ports.startGeneration({
        typeId: snapshot.kind === "ai" ? "qa" : snapshot.kind,
        studyModeId: modes[snapshot.kind], noteTitle: snapshot.noteTitle,
        sourceText: current.scope === "selection" ? snapshot.selection!.excerpt : snapshot.noteContent,
        requestedCount: current.requestedCount,
        context: { vaultPath: snapshot.vaultPath, notePath: snapshot.notePath, noteHash, selection: current.scope === "selection" ? snapshot.selection : null },
      });
      if (!isRunning(session)) { await cancelDetached(taskId); return; }
      current.taskId = taskId;
      if (current.stopping) {
        try { receive(await ports.cancelGeneration(taskId), session); }
        catch (error) {
          if (isRunning(session)) { current.stopping = false; ports.showToast(resolveError(error)); }
        }
      }
      schedulePoll(taskId, session);
    } catch (error) {
      if (!isRunning(session)) return;
      current.step = "scope";
      current.stopping = false;
      ports.showToast(resolveError(error));
    }
  }

  /** 停止仅请求后端结束当前运行，终态仍展示已生成草稿。 */
  async function stop(): Promise<void> {
    if (state.value.step !== "running" || state.value.stopping) return;
    state.value.stopping = true;
    const session = revision;
    const taskId = state.value.taskId;
    if (!taskId) return;
    try { receive(await ports.cancelGeneration(taskId), session); }
    catch (error) {
      if (isRunning(session)) { state.value.stopping = false; ports.showToast(resolveError(error)); }
    }
    schedulePoll(taskId, session);
  }

  /** 删除或换库永久使该会话失效，保留草稿供用户检查。 */
  function invalidate(message: string): void {
    if (!state.value.open) return;
    state.value.invalidReason = message;
    void stop();
  }

  /** 更改选区范围仅在开始前有效。 */
  function setScope(scope: "note" | "selection"): void {
    if (state.value.step === "scope" && (scope === "note" || state.value.snapshot?.selection)) state.value.scope = scope;
  }

  /** 数量入口接受 UI 提供的三个固定选项。 */
  function setCount(count: number): void {
    if (state.value.step === "scope" && [-1, 5, 10].includes(count)) state.value.requestedCount = count;
  }

  /** 勾选以 UUID 为键，其他草稿删除不会改变用户选择。 */
  function toggleAccepted(id: string): void {
    if (state.value.saving || !state.value.drafts.some((card) => card.draftId === id)) return;
    const selected = state.value.accepted;
    if (selected.has(id)) selected.delete(id); else selected.add(id);
  }

  /** 删除仅移除该草稿及其勾选，不重建全选集合。 */
  function removeDraft(id: string): void {
    if (state.value.saving) return;
    state.value.drafts = state.value.drafts.filter((card) => card.draftId !== id);
    state.value.accepted.delete(id);
  }

  /** 原子采纳成功后再刷新界面，刷新失败仍报告提交成功。 */
  async function adopt(): Promise<void> {
    const current = state.value;
    const snapshot = current.snapshot;
    const cards = current.drafts.filter((card) => current.accepted.has(card.draftId));
    if (!snapshot || current.saving || current.invalidReason || !cards.length) return;
    current.saving = true;
    let result: AdoptCardsResult;
    try {
      const expectedNoteHash = await ports.verifySnapshot(snapshot);
      if (current.invalidReason) throw new Error(current.invalidReason);
      result = await ports.adoptCards({ expectedVaultPath: snapshot.vaultPath, expectedNoteHash, notePath: snapshot.notePath, kind: snapshot.kind === "ai" ? "qa" : snapshot.kind, cards });
    } catch (error) {
      ports.showToast(resolveError(error));
      current.saving = false;
      return;
    }
    current.saving = false;
    close();
    const summary = `已新增 ${result.addedIds.length} 张，已存在 ${result.existingIds.length} 张，重复跳过 ${result.duplicateIds.length} 张`;
    try { await ports.refresh(snapshot); ports.showToast(summary); }
    catch (error) { ports.showToast(`${summary}；卡片已保存，刷新失败：${resolveError(error)}`); }
  }

  return { state, open, close, start, stop, invalidate, setScope, setCount, toggleAccepted, removeDraft, adopt };
}
