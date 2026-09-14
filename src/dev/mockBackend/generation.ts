import type { AdoptCardsInput, GenerationInput, GenerationTaskStatus } from "../../domain/types";
import { adoptCards, generateCards } from "./cards";

const tasks = new Map<string, GenerationTaskStatus>();

/** 演示任务只承载固定示例及协议形状，不复制真实生成或调度规则。 */
function start(args?: Record<string, unknown>): { taskId: string } {
  const input = args?.input as GenerationInput;
  const taskId = crypto.randomUUID();
  const result = generateCards(input);
  tasks.set(taskId, { taskId, state: "running", phase: "generating", generatedCount: result.cards.length, result, error: null });
  return { taskId };
}

/** 固定示例在第一次查询完成，保证 UI 使用真实任务返回值。 */
function status(args?: Record<string, unknown>): GenerationTaskStatus {
  const task = tasks.get(String(args?.taskId));
  if (!task) throw new Error("演示生成任务不存在");
  if (task.state === "running") task.state = "completed";
  return { ...task };
}

/** 停止演示任务时回传已有示例，供验证保留草稿交互。 */
function cancel(args?: Record<string, unknown>): GenerationTaskStatus {
  const task = tasks.get(String(args?.taskId));
  if (!task) throw new Error("演示生成任务不存在");
  if (task.state === "running") task.state = "cancelled";
  return { ...task };
}

/** 演示命令通过注册表扩展，避免在分发器增加命令分支。 */
export const generationCommands: Record<string, (args?: Record<string, unknown>) => unknown> = {
  start_generation: start,
  get_generation_status: status,
  cancel_generation: cancel,
  /** 原生成入口仍复用固定示例。 */
  generate_cards: (args) => generateCards(args?.input as GenerationInput),
  /** 采纳仅演示 UUID 和字段回显。 */
  adopt_cards: (args) => adoptCards(args?.input as AdoptCardsInput),
};
