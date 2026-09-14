import type { AgentRunState } from "../../domain/agent";

/**
 * 目标状态短词：头部 chip 与目标下拉面板共用同一套词，
 * 避免同一事实在两处出现不同说法；返回词一律极短，长句交给 goalPhaseHint。
 */
export function goalPhaseWord(
  goalPhase: string | null | undefined,
  runState: AgentRunState | null | undefined,
  busy: boolean,
  storedPhase?: string | null,
): string {
  const phase = goalPhase || storedPhase || "";
  if (phase === "complete") return "已完成";
  // 本轮快照优先于持久化阶段：失败/停止描述的是这一轮，不是目标终态。
  if (runState === "failed") return "本轮失败";
  if (runState === "cancelled") return "已停止";
  if (runState === "waiting") return "等待你";
  if (phase === "blocked" || runState === "blocked") return "受阻";
  if (phase === "paused" || runState === "paused") return "已暂停";
  // 打开已存 active 目标不等于已获得续轮许可，没有运行中的轮次即等待用户。
  if (phase === "active") return busy ? "进行中" : "等待你";
  return phase;
}

/**
 * 说明性长句：只允许进 title/aria-label 或展开面板里的 hint，
 * 绝不能当 chip 文本或面板标题，否则头部会被解释性文字撑开。
 */
export function goalPhaseHint(
  goalPhase: string | null | undefined,
  runState: AgentRunState | null | undefined,
  busy: boolean,
): string {
  if (goalPhase === "complete") return "";
  if (runState === "failed") return "本轮失败 · 目标未完成";
  if (runState === "cancelled") return "已停止 · 目标未完成";
  if (goalPhase === "active" && !busy) return "等待继续 · 目标未完成";
  return "";
}
