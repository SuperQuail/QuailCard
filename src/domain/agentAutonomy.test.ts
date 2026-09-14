import { expect, test } from "vitest";
import type { AgentRun, AgentRunState, AgentSession, GoalPhase, PlanStatus } from "./agent";
import { projectAgentAutonomy, projectAgentAutonomyRun, projectAgentGoal, projectAgentPlan } from "./agentAutonomy";

/** 旧版本会话刻意省略所有自主执行字段，确保现有调用方无需补造数据。 */
function legacySession(): AgentSession {
  return {
    formatVersion: 1, id: "root", title: "会话", updatedAt: 1, summary: "", selectedPaths: [],
    messages: [{ id: "m1", role: "assistant", content: "本轮结束", kind: "text", data: null }],
  };
}

/** 运行快照保留旧契约形状，仅在用例中明确提供新增字段。 */
function run(state: AgentRunState, extra: Partial<AgentRun> = {}): AgentRun {
  return {
    id: "run1", sessionId: "root", state, sequence: 1, text: "", phase: "",
    pendingWrite: null, pendingWriteId: null, error: null, reasoning: "", reasoningMessageId: "",
    ...extra,
  };
}

/** 完整领域对象经默认投影构造，以便每个用例只突出阶段差异。 */
function sessionWithGoal(phase: GoalPhase = "active"): AgentSession {
  return { ...legacySession(), goal: projectAgentGoal({ id: "goal1", phase, objective: "完成资料核验" }) };
}

test("旧会话和空输入默认兼容，不从历史文本伪造 Goal、Plan 或完整轮次边界", () => {
  const view = projectAgentAutonomy(legacySession());
  expect(view).toEqual({
    goal: null, plan: { ownerSessionId: "", revision: 0, steps: [] },
    planCounts: { pending: 0, in_progress: 0, completed: 0, blocked: 0, cancelled: 0 },
    parentSessionId: null, delegationDepth: 0, completedMessageCount: 0, children: [],
    runState: null, goalPhase: null, waitingReason: null, busy: false, turnEnded: false,
    goalComplete: false, ordinaryTurnComplete: false,
  });
  expect(projectAgentAutonomy()).toEqual(view);
  expect(projectAgentAutonomy(null)).toEqual(view);
});

test("Rust 缺字段默认值保持保守：Goal paused、步骤 required=true、空计划 owner 不伪造", () => {
  expect(projectAgentGoal({})).toEqual({
    id: "", revision: 0, objective: "", acceptanceCriteria: [], phase: "paused",
    roundsStarted: 0, maxGoalRounds: 0, evidence: [], blocker: null,
  });
  expect(projectAgentGoal(null)).toBeNull();
  expect(projectAgentPlan({ steps: [{}] })).toEqual({
    ownerSessionId: "", revision: 0, steps: [{
      id: "", content: "", status: "pending", required: true,
      dependencies: [], childAgentId: null, resultRefs: [],
    }],
  });
});

test.each<AgentRunState>(["running", "waiting", "paused", "blocked", "completed", "cancelled", "failed"])(
  "只有 running 为 busy，%s 的轮次结束不等于活跃 Goal 完成", state => {
    const view = projectAgentAutonomy(sessionWithGoal(), run(state));
    expect(view.busy).toBe(state === "running");
    expect(view.turnEnded).toBe(state !== "running");
    expect(view.goalComplete).toBe(false);
    expect(view.ordinaryTurnComplete).toBe(false);
  },
);

test.each<GoalPhase>(["active", "paused", "blocked", "complete"])(
  "Goal %s 只认宿主阶段，不将 completed 运行快照当总目标成功", phase => {
    const view = projectAgentAutonomy(sessionWithGoal(phase), run("completed"));
    expect(view.goalPhase).toBe(phase);
    expect(view.goalComplete).toBe(phase === "complete");
    expect(view.ordinaryTurnComplete).toBe(false);
    expect(view.turnEnded).toBe(true);
  },
);

test("旧普通会话 completed 只说明普通轮次成功，取消与失败不冒充成功", () => {
  expect(projectAgentAutonomy(legacySession(), run("completed"))).toMatchObject({
    turnEnded: true, goalComplete: false, ordinaryTurnComplete: true,
  });
  for (const state of ["cancelled", "failed", "waiting", "paused", "blocked"] as const) {
    expect(projectAgentAutonomy(legacySession(), run(state)).ordinaryTurnComplete).toBe(false);
  }
});

test("等待用户已经结束本轮；等待子任务仍沿用 running，不借等待原因改写 busy", () => {
  expect(projectAgentAutonomy(sessionWithGoal(), run("waiting", { waitingReason: "waitingUser" })))
    .toMatchObject({ busy: false, turnEnded: true, goalComplete: false, waitingReason: "waitingUser" });
  expect(projectAgentAutonomy(sessionWithGoal(), run("running", { waitingReason: "waitingChildren" })))
    .toMatchObject({ busy: true, turnEnded: false, goalComplete: false, waitingReason: "waitingChildren" });
});

test("当前 run 的阶段覆盖持久化旧阶段，空字段回退，未知阶段不误判成功", () => {
  expect(projectAgentAutonomy(sessionWithGoal(), run("completed", { goalPhase: "complete" })))
    .toMatchObject({ goalPhase: "complete", goalComplete: true, ordinaryTurnComplete: false });
  expect(projectAgentAutonomy(sessionWithGoal("complete"), run("paused", { goalPhase: "paused" })))
    .toMatchObject({ goalPhase: "paused", goalComplete: false });
  expect(projectAgentAutonomy(sessionWithGoal(), run("completed", { goalPhase: "" })).goalPhase).toBe("active");
  expect(projectAgentAutonomy(legacySession(), run("completed", { goalPhase: "futurePhase" })))
    .toMatchObject({ goalPhase: "futurePhase", goalComplete: false, ordinaryTurnComplete: false });
});

test("跨会话运行快照与无会话快照均丢弃，恢复 active Goal 不暗示执行或续轮许可", () => {
  const foreignRun = run("completed", { sessionId: "child", goalPhase: "complete", waitingReason: "old" });
  expect(projectAgentAutonomy(sessionWithGoal(), foreignRun)).toMatchObject({
    runState: null, goalPhase: "active", waitingReason: null, busy: false, turnEnded: false, goalComplete: false,
  });
  expect(projectAgentAutonomy(null, foreignRun).goalComplete).toBe(false);
  const restored = projectAgentAutonomy(sessionWithGoal());
  expect(restored.busy).toBe(false);
  expect(restored).not.toHaveProperty("armed");
  expect(restored).not.toHaveProperty("shouldContinue");
});

test("持久化计划保留并行、取消、阻塞、依赖与子报告引用，历史旧计划消息不覆盖它", () => {
  const statuses: PlanStatus[] = ["pending", "in_progress", "in_progress", "completed", "blocked", "cancelled"];
  const session = sessionWithGoal();
  session.plan = projectAgentPlan({ ownerSessionId: "root", revision: 4, steps: statuses.map((status, index) => ({
    id: `s${index}`, content: "步骤", status, dependencies: ["upstream"],
    childAgentId: "child", resultRefs: ["receipt:1"], required: index !== 5,
  })) });
  session.messages.push({ id: "old", role: "tool", kind: "plan", content: "", data: { steps: [] } });
  const view = projectAgentAutonomy(session);
  expect(view.plan).toEqual(session.plan);
  expect(view.planCounts).toEqual({ pending: 1, in_progress: 2, completed: 1, blocked: 1, cancelled: 1 });
  expect(view.plan.steps[5].required).toBe(false);
  expect(view.goalComplete).toBe(false);
  session.plan.steps.forEach(step => { step.status = "completed"; });
  expect(projectAgentAutonomy(session, run("completed")).goalComplete).toBe(false);
});

test("投影拷贝全部可变嵌套数据，改动输出不污染原始 Goal、Plan 或父子关系", () => {
  const session = sessionWithGoal("blocked");
  session.goal = projectAgentGoal({
    ...session.goal!, revision: 7, acceptanceCriteria: ["验收"], roundsStarted: 3, maxGoalRounds: 8,
    evidence: [{ criterionIndex: 0, goalRevision: 6, sourceVersion: "hash:1", receiptRef: "receipt:1" }],
    blocker: { reason: "权限不足", attempts: [{ round: 3, resultRefs: ["attempt:3"] }] },
  });
  session.plan = projectAgentPlan({ steps: [{ dependencies: ["s1"], resultRefs: ["result:1"] }] });
  session.parentSessionId = "parent"; session.delegationDepth = 2;
  session.completedMessageCount = 1; session.children = ["child"];
  const before = JSON.stringify(session);
  const view = projectAgentAutonomy(session);
  expect(view).toMatchObject({ parentSessionId: "parent", delegationDepth: 2, completedMessageCount: 1 });
  expect(view.goal).toEqual(session.goal);
  view.goal!.acceptanceCriteria.push("changed");
  view.goal!.evidence[0].receiptRef = "changed";
  view.goal!.blocker!.reason = "changed";
  view.goal!.blocker!.attempts[0].resultRefs.push("changed");
  view.plan.steps[0].dependencies.push("changed");
  view.plan.steps[0].resultRefs.push("changed");
  view.children.push("changed");
  expect(JSON.stringify(session)).toBe(before);
  expect(projectAgentAutonomy(session)).toEqual(projectAgentAutonomy(session));
});

/** 文字序号变化不替换目标和计划对象，运行丢失后仍回退持久阶段。 */
test("实时投影复用静态引用且不残留其他运行的阶段", () => {
  const session = sessionWithGoal(), base = projectAgentAutonomy(session);
  const first = projectAgentAutonomyRun(base, session.id, run("running", { goalPhase: "blocked" }));
  const next = projectAgentAutonomyRun(base, session.id, run("running", { sequence: 2, text: "新片段" }));
  expect(first.goal).toBe(base.goal); expect(next.goal).toBe(base.goal);
  expect(next.plan).toBe(base.plan); expect(next.planCounts).toBe(base.planCounts); expect(next.children).toBe(base.children);
  expect(next.goalPhase).toBe("active");
  expect(projectAgentAutonomyRun(first, session.id, null).goalPhase).toBe("active");
  expect(projectAgentAutonomyRun(base, session.id, run("completed", { sessionId: "other" })).busy).toBe(false);
});
