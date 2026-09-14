//! Goal 与计划的持久化契约；运行许可不进入 JSON，版本熔断由外层会话存储负责。
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum GoalPhase {
    Active,
    #[default]
    Paused,
    Blocked,
    Complete,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Goal {
    pub id: String,
    pub revision: u64,
    pub objective: String,
    pub acceptance_criteria: Vec<String>,
    pub phase: GoalPhase,
    /// 展示计数达到 u32::MAX 后饱和，不限制继续执行。
    pub rounds_started: u32,
    /// 废弃惰性字段：保留旧 JSON 名称与类型，任何值都不再限制自动续轮。
    pub max_goal_rounds: u32,
    pub evidence: Vec<GoalEvidence>,
    pub blocker: Option<GoalBlocker>,
}

/// 收据是宿主可查询的引用，不把模型叙述当作副作用成功证明。
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct GoalEvidence {
    pub criterion_index: u32,
    /// 指向提交 complete 前的目标修订；完成动作本身仍递增 revision。
    pub goal_revision: u64,
    pub source_version: String,
    pub receipt_ref: String,
}

/// 同一障碍的语义连续性仍由主 Agent 判断，宿主只校验逐轮尝试的完整性。
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct GoalBlocker {
    pub reason: String,
    pub attempts: Vec<BlockerAttempt>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct BlockerAttempt {
    pub round: u32,
    pub result_refs: Vec<String>,
}

/// 计划属于一个会话；空默认值仅用于旧文件恢复，写入口必须验证 owner。
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Plan {
    pub owner_session_id: String,
    pub revision: u64,
    pub steps: Vec<PlanStep>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum PlanStatus {
    #[default]
    #[serde(rename = "pending")]
    Pending,
    #[serde(rename = "in_progress")]
    InProgress,
    #[serde(rename = "completed")]
    Completed,
    #[serde(rename = "blocked")]
    Blocked,
    #[serde(rename = "cancelled")]
    Cancelled,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct PlanStep {
    pub id: String,
    pub content: String,
    pub status: PlanStatus,
    /// 缺省必需，防止旧计划因缺字段绕过完成检查。
    pub required: bool,
    pub dependencies: Vec<String>,
    pub child_agent_id: Option<String>,
    /// 产物追踪引用可保留旧格式，不等同于 GoalEvidence 的可验证收据。
    pub result_refs: Vec<String>,
}

impl Default for PlanStep {
    /// 兼容旧文件时采用保守语义，未声明可选的步骤都必须完成。
    fn default() -> Self {
        Self {
            id: String::new(),
            content: String::new(),
            status: PlanStatus::Pending,
            required: true,
            dependencies: Vec::new(),
            child_agent_id: None,
            result_refs: Vec::new(),
        }
    }
}
