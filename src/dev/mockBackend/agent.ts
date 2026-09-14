import type { AgentSession, AgentRun, AgentInput } from "../../domain/agent";
const sessions: AgentSession[] = [];
let memory = "";
let revision = 0;
let run: AgentRun | null = null;
/** 演示只展示消息形态，不实现文件操作、模型调用或调度规则。 */
const handlers: Record<string, (args: Record<string, unknown>) => unknown> = {
  agent_sessions: () => sessions,
  agent_memory: () => ({ formatVersion: 1, content: memory }),
  agent_save_memory: args => ({ formatVersion: 1, content: memory = String(args.content) }),
  agent_create_session: () => {
    const session: AgentSession = { formatVersion: 1, id: crypto.randomUUID(), title: "新会话", updatedAt: Date.now() / 1000, messages: [], summary: "", selectedPaths: [] };
    sessions.unshift(session); return session;
  },
  agent_session: args => sessions.find(s => s.id === args.id),
  // 演示只移除内存列表，不复制真实文件回收流程。
  agent_delete_session: args => {
    const index = sessions.findIndex(session => session.id === args.id);
    if (index >= 0) sessions.splice(index, 1);
  },
  agent_observe_session: args => {
    if (args.childId) throw new Error("子代理详情需要桌面应用");
    const session = sessions.find(item => item.id === args.sessionId);
    if (!session) throw new Error("会话不存在");
    const version = `demo:${session.id}:${revision}`;
    return { sessionId: session.id, revision: version, session: args.knownRevision === version ? null : session, run: run?.sessionId === session.id ? run : null, writes: [] };
  },
  agent_send: args => {
    revision += 1;
    const input = args.input as AgentInput;
    const session = sessions.find(s => s.id === input.sessionId)!;
    session.title = input.content.slice(0, 24) || "图片对话";
    session.messages.push({ id: crypto.randomUUID(), role: "user", kind: "text", content: input.content, data: { images: input.images ?? [] } }, { id: crypto.randomUUID(), role: "assistant", kind: "text", content: "这是浏览器演示。请在桌面应用中配置模型，即可检索笔记、直接修改文件并开展学习。", data: null });
    run = { id: input.requestId, sessionId: input.sessionId, state: "completed", sequence: 1, text: "", phase: "演示完成", pendingWrite: null, pendingWriteId: null, error: null, reasoning: "", reasoningMessageId: "" }; return run;
  },
  agent_status: () => run,
  agent_cancel: () => { if (run) run.state = "cancelled"; },
  agent_save_review: () => undefined,
  /** 浏览器演示没有待保存写入。 */
  agent_pending_writes: () => [],
  /** 演示不等待真实编辑器保存，也不冒充写入协调。 */
  agent_acknowledge_write: () => { throw new Error("写入确认需要桌面应用"); },
  /** 浏览器不模拟代理调度，列表明确为空。 */
  agent_children: () => [],
  /** 演示没有持久子会话，不冒充已验证的真实父链。 */
  agent_child_session: () => { throw new Error("子代理详情需要桌面应用"); },
  /** 演示不假装中断真实任务。 */
  agent_interrupt_child: () => { throw new Error("子代理中断需要桌面应用"); },
  /** 演示不假装已投递真实追问。 */
  agent_message_child: () => { throw new Error("子代理追问需要桌面应用"); },
};
/** 未提供演示的命令明确报错，不能假装完成真实写入。 */
export async function call<T>(command: string, args: Record<string, unknown> = {}): Promise<T> {
  const handler = handlers[command];
  if (!handler) throw new Error("此操作需要桌面应用");
  return structuredClone(handler(args)) as T;
}
