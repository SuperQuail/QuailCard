import type {
  AgentRun, AgentRunState, AgentSession, Goal, Plan, PlanStatus, PlanStep,
} from "./agent";

/** 只呈现宿主事实，不推导续轮许可，也不替代 Rust 的完成验收。 */
export interface AgentAutonomyProjection {
  goal: Goal | null;
  plan: Plan;
  planCounts: Record<PlanStatus, number>;
  parentSessionId: string | null;
  delegationDepth: number;
  completedMessageCount: number;
  children: string[];
  runState: AgentRunState | null;
  /** 匹配会话的运行快照优先；未知阶段保留原文且不认作完成。 */
  goalPhase: string | null;
  waitingReason: string | null;
  busy: boolean;
  /** 只表示有一轮结束，waiting/paused/blocked 不是目标成功终态。 */
  turnEnded: boolean;
  goalComplete: boolean;
  ordinaryTurnComplete: boolean;
}

/** 缺字段取 Rust 默认值；拷贝嵌套引用以免展示层意外改写持久化快照。 */
export function projectAgentGoal(goal?: Partial<Goal> | null): Goal | null {
  if (!goal) return null;
  return {
    id: goal.id ?? "", revision: goal.revision ?? 0, objective: goal.objective ?? "",
    acceptanceCriteria: [...(goal.acceptanceCriteria ?? [])],
    phase: goal.phase ?? "paused", roundsStarted: goal.roundsStarted ?? 0,
    maxGoalRounds: goal.maxGoalRounds ?? 0,
    evidence: (goal.evidence ?? []).map(evidence => ({ ...evidence })),
    blocker: goal.blocker ? {
      reason: goal.blocker.reason ?? "",
      attempts: (goal.blocker.attempts ?? []).map(attempt => ({
        round: attempt.round ?? 0, resultRefs: [...(attempt.resultRefs ?? [])],
      })),
    } : null,
  };
}

/** 缺省必需项保持 required=true；取消和阻塞不会被折算成 completed。 */
function projectPlanStep(step: Partial<PlanStep>): PlanStep {
  return {
    id: step.id ?? "", content: step.content ?? "", status: step.status ?? "pending",
    required: step.required ?? true, dependencies: [...(step.dependencies ?? [])],
    childAgentId: step.childAgentId ?? null, resultRefs: [...(step.resultRefs ?? [])],
  };
}

/** 旧会话得到空计划，不从历史 update_plan 消息重建或替换当前计划。 */
export function projectAgentPlan(
  plan?: Partial<Omit<Plan, "steps">> & { steps?: Partial<PlanStep>[] } | null,
): Plan {
  return {
    ownerSessionId: plan?.ownerSessionId ?? "", revision: plan?.revision ?? 0,
    steps: (plan?.steps ?? []).map(projectPlanStep),
  };
}

/** 并行项逐项计数，保持原顺序、依赖和子关联，不把计划完成当成 Goal 完成。 */
function countPlanSteps(plan: Plan): Record<PlanStatus, number> {
  const counts: Record<PlanStatus, number> = {
    pending: 0, in_progress: 0, completed: 0, blocked: 0, cancelled: 0,
  };
  for (const step of plan.steps) counts[step.status] += 1;
  return counts;
}

/** 静态投影仅在持久会话变化时构造，流式更新复用目标、计划与证据引用。 */
type AgentAutonomyFacts = Pick<AgentAutonomyProjection,
  "goal" | "plan" | "planCounts" | "parentSessionId" | "delegationDepth" | "completedMessageCount" | "children">;

/** 合并匹配会话的运行事实，不重新拷贝静态字段，也不继承上一执行的阶段。 */
export function projectAgentAutonomyRun(
  base: AgentAutonomyFacts, sessionId?: string | null, run?: AgentRun | null,
): AgentAutonomyProjection {
  const currentRun = sessionId && run?.sessionId === sessionId ? run : null;
  const runState = currentRun?.state ?? null;
  const goalPhase = currentRun?.goalPhase || base.goal?.phase || null;
  return {
    ...base, runState, goalPhase, waitingReason: currentRun?.waitingReason ?? null,
    busy: runState === "running",
    turnEnded: runState !== null && runState !== "running",
    goalComplete: goalPhase === "complete",
    ordinaryTurnComplete: runState === "completed" && goalPhase === null,
  };
}

/** 兼容原纯函数调用；打开已存 active Goal 不等于自动执行，跨会话运行仍被丢弃。 */
export function projectAgentAutonomy(
  session?: AgentSession | null, run?: AgentRun | null,
): AgentAutonomyProjection {
  const goal = projectAgentGoal(session?.goal), plan = projectAgentPlan(session?.plan);
  return projectAgentAutonomyRun({
    goal, plan, planCounts: countPlanSteps(plan),
    parentSessionId: session?.parentSessionId ?? null,
    delegationDepth: session?.delegationDepth ?? 0,
    completedMessageCount: session?.completedMessageCount ?? 0,
    children: [...(session?.children ?? [])],
  }, session?.id, run);
}
