/** 后端 Agent 会话契约；资料范围为空表示整个当前知识库。 */
export interface AgentSession {
  formatVersion: number; id: string; title: string; updatedAt: number;
  messages: AgentMessage[]; summary: string; selectedPaths: string[];
  /** 父级授予的写入范围；空或缺省表示只读，元素以 "/" 结尾表示目录前缀。 */
  writeScope?: string[];
  /** 新增字段可缺省以兼容旧会话；续轮许可不属于持久化契约。 */
  goal?: Goal | null; plan?: Plan; parentSessionId?: string | null;
  delegationDepth?: number; completedMessageCount?: number; children?: string[];
}
/** 消息显示数据不包含供应商凭据与推理内容。 */
export interface AgentMessage {
  id: string; role: string; content: string; kind: string;
  data: Record<string, unknown> | null;
}
/** 请求身份在网络重试时保持不变。 */
export interface AgentInput {
  sessionId: string; requestId: string; content: string; providerId: string; selectedPaths: string[];
  images?: NonNullable<import("./generation").GenerationInput["images"]>;
}
/** 完整状态快照通过 sequence 防止迟到覆盖。 */
export interface AgentRun {
  textMessageId?: string;
  id: string; sessionId: string; state: AgentRunState;
  goalPhase?: string; waitingReason?: string | null;
  sequence: number; text: string; phase: string; pendingWrite: string | null; pendingWriteId: string | null; error: string | null;
  /** 当前步骤的推理文本与它对应的持久化消息身份。 */
  reasoning: string; reasoningMessageId: string;
}

/** 待保存写入只含执行身份与操作身份，前端不接触工具参数或文件正文。 */
export interface AgentPendingWrite {
  executionId: string; sessionId: string; path: string; operationId: string;
}
/** 观察只返回变化的持久历史，实时快照与历史版本互相独立。 */
export interface AgentObservation {
  sessionId: string; revision: string; session: AgentSession | null; run: AgentRun | null;
  /** 根观察附带整树待编辑器保存的写入；子观察为空。 */
  writes: AgentPendingWrite[];
}
/** 后端白名单摘要不携带原始参数、结果正文或供应商重放。 */
export interface AgentToolDetail { label: string; value: string }
export interface AgentToolRow {
  id: string; name: string; state: "running" | "ok" | "error"; summary: string;
  details?: AgentToolDetail[]; errorCode?: string | null;
}
export interface AgentToolCalls { rows: AgentToolRow[] }

/** 子详情仅持有展示状态，不共享父会话的发送、草稿或执行控制。 */
export interface AgentChildDetailState {
  id: string; session: AgentSession | null; run: AgentRun | null; loading: boolean; error: string;
}

/** running 是唯一忙碌态；等待、暂停与阻塞均不表示目标完成。 */
export type AgentRunState = "running" | "waiting" | "paused" | "blocked" | "completed" | "cancelled" | "failed";
export type GoalPhase = "active" | "paused" | "blocked" | "complete";
export type PlanStatus = "pending" | "in_progress" | "completed" | "blocked" | "cancelled";

/** 与 agent_autonomy_models.rs 对齐；完成由宿主验收，前端不重做验收规则。 */
export interface Goal {
  id: string; revision: number; objective: string; acceptanceCriteria: string[];
  phase: GoalPhase; roundsStarted: number;
  /** @deprecated 仅保留旧持久化契约，任何值都不限制自动续轮。 */
  maxGoalRounds: number;
  evidence: GoalEvidence[]; blocker: GoalBlocker | null;
}
/** 收据引用及版本来自宿主，不以模型叙述代替实际副作用。 */
export interface GoalEvidence {
  criterionIndex: number; goalRevision: number; sourceVersion: string; receiptRef: string;
}
export interface GoalBlocker { reason: string; attempts: BlockerAttempt[] }
export interface BlockerAttempt { round: number; resultRefs: string[] }
/** 每个会话独占计划；整体替换以 revision 防止迟到更新。 */
export interface Plan { ownerSessionId: string; revision: number; steps: PlanStep[] }
/** resultRefs 仅追踪产物来源，不等同于 GoalEvidence 的可验证收据。 */
export interface PlanStep {
  id: string; content: string; status: PlanStatus; required: boolean;
  dependencies: string[]; childAgentId: string | null; resultRefs: string[];
}

/** 图片与生成接口共用格式，内容仅随用户发送进入后端。 */
export type AgentImage = NonNullable<AgentInput["images"]>[number];
/** 差异与撤销以持久化操作身份定位。 */
export interface AgentChange {
  formatVersion: number; id: string; path: string; before: string | null; after: string;
  beforeHash: string | null; afterHash: string; state: string;
}
/** 用户明确保存的跨会话偏好。 */
export interface AgentMemory { formatVersion: number; content: string }

/** 子代理只暴露持久身份与运行状态；ready 表示可由父模型冷恢复。 */
export interface AgentChildInfo {
  agentId: string; parentSessionId: string; delegationDepth: number;
  description: string; status: "running" | "idle" | "ready";
  /** 该子代理的写入授权，便于父级界面追溯；空表示只读。 */
  writeScope?: string[];
}
