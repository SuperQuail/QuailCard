import { invoke } from "@tauri-apps/api/core";
import { isTauri } from "./backend";
import type { AgentChange, AgentChildInfo, AgentInput, AgentMemory, AgentObservation, AgentPendingWrite, AgentRun, AgentSession } from "../domain/agent";

/** 浏览器仅使用独立演示数据，真实工具执行始终在后端。 */
async function call<T>(command: string, args?: Record<string, unknown>): Promise<T> {
  if (isTauri()) return invoke<T>(command, args);
  return (await import("../dev/mockBackend/agent")).call<T>(command, args);
}
/** 查询当前知识库的会话摘要。 */
export function sessions(): Promise<AgentSession[]> { return call("agent_sessions"); }
/** 创建持久会话。 */
export function createSession(): Promise<AgentSession> { return call("agent_create_session"); }
/** 读取会话完整历史。 */
export function session(id: string): Promise<AgentSession> { return call("agent_session", { id }); }
/** 后端合并持久子树与当前运行树；省略 runId 时只读取持久状态。 */
export function children(sessionId: string, runId?: string | null): Promise<AgentChildInfo[]> { return call("agent_children", { sessionId, runId: runId ?? null }); }
/** 后端验证父链后才允许读取子会话，不能绕过为普通会话读取。 */
export function childSession(sessionId: string, childId: string): Promise<AgentSession> { return call("agent_child_session", { sessionId, childId }); }
/** 只读观察带历史版本，未变化时不重复传输全量消息。 */
export function observe(sessionId: string, runId?: string | null, childId?: string | null, knownRevision?: string | null): Promise<AgentObservation> {
  return call("agent_observe_session", { sessionId, runId: runId ?? null, childId: childId ?? null, knownRevision: knownRevision ?? null });
}
/** 仅中断当前活动树中的子任务，不删除其持久会话。 */
export function interruptChild(runId: string, childId: string): Promise<void> { return call("agent_interrupt_child", { runId, childId }); }
/** 活动树直接子追问由宿主校验；闲置 root 必须另走人类聊天请求。 */
export function messageChild(runId: string, childId: string, message: string): Promise<void> { return call("agent_message_child", { runId, childId, message }); }
/** 删除仅针对会话记录，笔记与卡片保留。 */
export function deleteSession(id: string): Promise<void> { return call("agent_delete_session", { id }); }
/** 后端登记完成才返回任务身份。 */
export function send(input: AgentInput): Promise<AgentRun> { return call("agent_send", { input }); }
/** 查询完整快照供重连恢复。 */
export function status(id: string): Promise<AgentRun> { return call("agent_status", { id }); }
/** 停止未完成工作，保留已完成改动。 */
export function cancel(id: string): Promise<void> { return call("agent_cancel", { id }); }
/** 只读整树待保存写入；观察失败时据此完成保存协调，避免子代理死等。 */
export function pendingWrites(id: string): Promise<AgentPendingWrite[]> { return call("agent_pending_writes", { id }); }
/** 编辑器保存完成后确认整树中的指定执行，等待真实写入结束。 */
export function acknowledgeWrite(id: string, executionId: string, operationId: string): Promise<void> {
  return call("agent_acknowledge_write", { id, executionId, operationId });
}
/** 读取真实修改状态与恢复内容。 */
export function change(id: string): Promise<AgentChange> { return call("agent_change", { id }); }
/** 后端核对当前文件后撤销指定操作。 */
export function undo(id: string): Promise<AgentChange> { return call("agent_undo", { id }); }
/** 读取显式长期记忆。 */
export function memory(): Promise<AgentMemory> { return call("agent_memory"); }
/** 保存或清空长期记忆。 */
export function saveMemory(content: string): Promise<AgentMemory> { return call("agent_save_memory", { content }); }
/** 仅保存内嵌复习的界面进度，不产生评分。 */
export function saveReview(sessionId: string, messageId: string, progress: Record<string, unknown>): Promise<void> { return call("agent_save_review", { sessionId, messageId, progress }); }
/** 仅提交用户选中的草稿 ID，实际内容由后端会话恢复。 */
export function adoptCards(sessionId: string, messageId: string, draftIds: string[]): Promise<AgentSession> { return call("agent_adopt_cards", { sessionId, messageId, draftIds }); }
