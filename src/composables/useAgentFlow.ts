import { deleteAgentSession, agentState, openAgent, sendAgentMessage, selectAgentSession, stopAgent, showAgentChange, undoAgent, saveAgentMemory, adoptAgentCards } from "../services/stores/agentStore";
import { agentReviewFlow } from "../services/agentReview";
import { notes, activeNotePath } from "../services/stores/noteStore";
import type { AgentMessage } from "../domain/agent";
import { resolveError } from "../utils/errorMessage";
import { agentChildDetail, agentChildren, pasteAgentImages, removeAgentImage } from "../services/stores/agentStore";

/** 工作区只将受控展示事件交给用例，不接触后端命令或全量应用状态。 */
export function useAgentFlow(options: { selectNote: (path: string) => Promise<void>; openSplit: () => void; showToast: (message: string) => void }) {
  /** 引用必须指向知识库索引中的笔记，不能用模型链接打开内部文件。 */
  async function openNote(path: string): Promise<void> {
    if (!notes.value.some(note => note.path === path)) throw new Error("引用的笔记不存在，请刷新或让 Agent 重新检索");
    await options.selectNote(path);
    if (activeNotePath.value === path) agentState.open = false;
  }
  /** 生成入口先固定真实笔记，再打开已有草稿生成与采纳流程。 */
  async function generate(path: string): Promise<void> { await openNote(path); if (activeNotePath.value === path) options.openSplit(); }
  /** 记忆建议只能追加到用户可见的长期记忆，不能替换隐藏状态。 */
  async function remember(content: string): Promise<void> { await saveAgentMemory([agentState.memory, content].filter(Boolean).join("\n")); }
  const actions: Record<string, (value: string) => Promise<void>> = { note: openNote, change: showAgentChange, generate, memory: remember };
  /** 注册表只接收已定义动作；未知动作显示错误而不解释为代码。 */
  async function action(name: string, value: string): Promise<void> {
    try { const handler = actions[name]; if (!handler) throw new Error("不支持的 Agent 动作"); await handler(value); }
    catch (error) { options.showToast(resolveError(error)); }
  }
  /** 消息身份绑定复习状态，切换笔记后可继续同一轮。 */
  function reviewFlow(message: AgentMessage) { return agentReviewFlow(agentState.session!.id, message); }
  /** 停止失败保留任务状态供用户重试。 */
  async function stop(): Promise<void> { try { await stopAgent(); } catch (error) { agentState.error = resolveError(error); } }
  return { ...agentChildDetail, refreshChildren: agentChildren.refreshChildren, interruptChild: agentChildren.interruptChild, messageChild: agentChildren.messageChild, resumeGoal: agentChildren.resumeGoal, state: agentState, pasteImages: pasteAgentImages, removeImage: removeAgentImage, open: openAgent, send: sendAgentMessage, select: selectAgentSession, deleteSession: deleteAgentSession, stop, action, reviewFlow, undo: undoAgent, saveMemory: saveAgentMemory, adopt: adoptAgentCards };
}
