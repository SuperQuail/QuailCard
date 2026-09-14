/** Agent 工具调用行的展示模型；只解析会话里已持久化的 exchange 块。 */
import type { AgentMessage } from "./agent";

export interface AgentToolCallRow {
  id: string; name: string; arguments: string; output: string | null;
  state: "running" | "ok" | "error";
  /** 折叠时的一行摘要：出错是错误首行，执行中是占位文案，其余是参数摘要。 */
  summary: string;
}

interface ReplayCall { id: string; name: string; arguments: string }

/** 只识别两种协议已持久化的调用形状，未知形状不猜测。 */
function replayCalls(replay: unknown): ReplayCall[] {
  const calls: ReplayCall[] = [];
  const assistant = replay as { tool_calls?: unknown; responseItems?: unknown } | null;
  if (Array.isArray(assistant?.tool_calls)) {
    for (const item of assistant.tool_calls) {
      const call = item as { id?: unknown; function?: { name?: unknown; arguments?: unknown } };
      if (typeof call.id === "string" && typeof call.function?.name === "string") {
        calls.push({ id: call.id, name: call.function.name, arguments: typeof call.function.arguments === "string" ? call.function.arguments : "" });
      }
    }
  }
  if (Array.isArray(assistant?.responseItems)) {
    for (const item of assistant.responseItems) {
      const call = item as { type?: unknown; call_id?: unknown; id?: unknown; name?: unknown; arguments?: unknown };
      if (call.type !== "function_call" || typeof call.name !== "string") continue;
      const id = typeof call.call_id === "string" ? call.call_id : call.id;
      if (typeof id === "string") {
        calls.push({ id, name: call.name, arguments: typeof call.arguments === "string" ? call.arguments : "" });
      }
    }
  }
  return calls;
}

/** 参数摘要把对象压成 k=v，读不动的原文只截断展示。 */
function argumentsSummary(raw: string): string {
  try {
    const value = JSON.parse(raw) as Record<string, unknown>;
    if (!value || typeof value !== "object" || Array.isArray(value)) return raw.slice(0, 80);
    return Object.entries(value)
      .map(([key, item]) => `${key}=${typeof item === "string" ? item : JSON.stringify(item)}`)
      .join(", ")
      .slice(0, 80);
  } catch {
    return raw.slice(0, 80);
  }
}

/** 后端结果信封只认固定字段；解析失败按失败展示，不隐藏原始输出。 */
function resultState(output: string | null): { state: "running" | "ok" | "error"; text: string | null } {
  if (!output) return { state: "running", text: null };
  try {
    const value = JSON.parse(output) as { ok?: unknown; status?: unknown; error?: { message?: unknown } };
    if (value.status === "notExecuted") return { state: "running", text: null };
    if (value.ok === true) return { state: "ok", text: null };
    return { state: "error", text: typeof value.error?.message === "string" ? value.error.message : "工具执行失败" };
  } catch {
    return { state: "error", text: output.split("\n", 1)[0] ?? output };
  }
}

/** 把一条 exchange 消息转成工具调用行；其他消息返回空数组。 */
export function agentToolCallRows(message: AgentMessage): AgentToolCallRow[] {
  if (message.kind !== "exchange") return [];
  const data = message.data as { assistant?: unknown; results?: unknown } | null;
  if (!data || !Array.isArray(data.results)) return [];
  const outputs = new Map<string, string>();
  for (const item of data.results) {
    const result = item as { tool_call_id?: unknown; content?: unknown };
    if (typeof result.tool_call_id === "string" && typeof result.content === "string") {
      outputs.set(result.tool_call_id, result.content);
    }
  }
  return replayCalls(data.assistant).map(call => {
    const output = outputs.get(call.id) ?? null;
    const { state, text } = resultState(output);
    const summary = state === "error"
      ? (text ?? "工具执行失败").split("\n", 1)[0] ?? ""
      : state === "running"
        ? "执行中…"
        : argumentsSummary(call.arguments);
    return { id: call.id, name: call.name, arguments: call.arguments, output, state, summary };
  });
}
