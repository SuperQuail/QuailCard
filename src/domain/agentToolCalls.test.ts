import { expect, test } from "vitest";
import { agentToolCallRows } from "./agentToolCalls";
import type { AgentMessage } from "./agent";

function exchange(data: Record<string, unknown>): AgentMessage {
  return { id: "e1", role: "assistant", kind: "exchange", content: "工具记录", data };
}

test("OpenAI 形状按 id 配对调用与结果，参数摘要清晰", () => {
  const rows = agentToolCallRows(exchange({
    assistant: { tool_calls: [{ id: "call_1", type: "function", function: { name: "read_note", arguments: "{\"path\":\"a.md\"}" } }] },
    results: [{ role: "tool", tool_call_id: "call_1", content: "{\"ok\":true,\"result\":{\"state\":\"ok\"}}" }],
  }));
  expect(rows).toHaveLength(1);
  expect(rows[0]).toMatchObject({ name: "read_note", state: "ok", summary: "path=a.md" });
});

test("Responses 形状的 function_call 同样能配对，未执行结果是执行中", () => {
  const rows = agentToolCallRows(exchange({
    assistant: { responseItems: [{ type: "function_call", call_id: "call_2", name: "emit_card", arguments: "{\"kind\":\"qa\"}" }] },
    results: [{ role: "tool", tool_call_id: "call_2", content: "{\"ok\":false,\"status\":\"notExecuted\"}" }],
  }));
  expect(rows[0]).toMatchObject({ name: "emit_card", state: "running", summary: "执行中…" });
});

test("失败结果折叠摘要只取错误首行，未知形状不猜调用", () => {
  const rows = agentToolCallRows(exchange({
    assistant: { tool_calls: [{ id: "call_3", function: { name: "edit_note", arguments: "{}" } }] },
    results: [{ tool_call_id: "call_3", content: "{\"ok\":false,\"error\":{\"code\":\"SOURCE_NOT_FOUND\",\"message\":\"摘录不存在\\n请重试\"}}" }],
  }));
  expect(rows[0]).toMatchObject({ state: "error", summary: "摘录不存在" });
  expect(agentToolCallRows(exchange({ assistant: { unknown: true }, results: [] }))).toEqual([]);
  expect(agentToolCallRows({ id: "t", role: "assistant", kind: "text", content: "x", data: null })).toEqual([]);
});
