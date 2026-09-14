use super::*;
use crate::agent_autonomy_models::*;

/// 用最小有效目标隔离测试，不依赖宿主或存储。
fn goal() -> Goal {
    create_goal(None, Authority::RootUser, "goal-1", spec()).unwrap()
}

/// 零惰性值与新目标一致，不授予或限制运行许可。
fn spec() -> GoalSpec {
    GoalSpec {
        objective: "整理笔记".into(),
        acceptance_criteria: vec!["已保存".into()],
        max_goal_rounds: 0,
    }
}

/// 构造完整步骤，required 默认必须保守。
fn step(id: &str, status: PlanStatus) -> PlanStep {
    PlanStep {
        id: id.into(),
        content: "核对结果".into(),
        status,
        ..PlanStep::default()
    }
}

/// 成功状态只在宿主明确无待处理事实时有效。
fn context(plan: &Plan) -> CompletionContext<'_> {
    CompletionContext {
        plan,
        owner_session_id: "root",
        pending_children: 0,
        pending_messages: 0,
        descendants_settled: true,
        source_version: "v1",
    }
}

/// 旧文件缺字段和未来未知字段容忍，保存仍完整输出当前字段。
#[test]
fn serde_defaults_and_complete_wire() {
    let goal: Goal = serde_json::from_str(r#"{"unknown":true}"#).unwrap();
    assert_eq!(goal.phase, GoalPhase::Paused);
    let value = serde_json::to_value(goal).unwrap();
    for key in [
        "id",
        "revision",
        "objective",
        "acceptanceCriteria",
        "phase",
        "roundsStarted",
        "maxGoalRounds",
        "evidence",
        "blocker",
    ] {
        assert!(value.get(key).is_some());
    }
    assert!(value.get("unknown").is_none());
    assert!(value.get("armed").is_none());
    let step: PlanStep = serde_json::from_str("{}").unwrap();
    assert!(step.required);
    assert_eq!(
        serde_json::to_value(PlanStatus::InProgress).unwrap(),
        "in_progress"
    );
    assert_eq!(serde_json::from_str::<Plan>("{}").unwrap(), Plan::default());
}

/// 外部资料、自动轮次和子消息不能创造用户授权。
#[test]
fn creation_authority_and_bounds() {
    for authority in [
        Authority::RootAutomatic,
        Authority::Child,
        Authority::External,
    ] {
        assert_eq!(
            create_goal(None, authority, "id", spec()).unwrap_err(),
            DomainError::Unauthorized
        );
    }
    assert_eq!(
        create_goal(Some(&goal()), Authority::RootUser, "id", spec()).unwrap_err(),
        DomainError::ExistingGoal
    );
    let mut input = spec();
    input.acceptance_criteria.clear();
    assert!(create_goal(None, Authority::RootUser, "id", input).is_err());
}

/// CAS 与权限失败保持原始快照，只有直接用户可以暂停和恢复。
#[test]
fn lifecycle_cas_and_authority() {
    let original = goal();
    assert_eq!(
        update_goal(
            &original,
            "wrong",
            1,
            Authority::RootUser,
            GoalUpdate::Pause,
            None,
            |_| false
        )
        .unwrap_err(),
        DomainError::Conflict
    );
    assert_eq!(
        update_goal(
            &original,
            "goal-1",
            0,
            Authority::RootUser,
            GoalUpdate::Pause,
            None,
            |_| false
        )
        .unwrap_err(),
        DomainError::Conflict
    );
    for authority in [
        Authority::RootAutomatic,
        Authority::Child,
        Authority::External,
    ] {
        assert_eq!(
            update_goal(
                &original,
                "goal-1",
                1,
                authority,
                GoalUpdate::Pause,
                None,
                |_| false
            )
            .unwrap_err(),
            DomainError::Unauthorized
        );
    }
    let paused = update_goal(
        &original,
        "goal-1",
        1,
        Authority::RootUser,
        GoalUpdate::Pause,
        None,
        |_| false,
    )
    .unwrap();
    assert_eq!(paused.phase, GoalPhase::Paused);
    assert_eq!(paused.revision, 2);
    let resumed = update_goal(
        &paused,
        "goal-1",
        2,
        Authority::RootUser,
        GoalUpdate::Resume,
        None,
        |_| false,
    )
    .unwrap();
    assert_eq!(resumed.phase, GoalPhase::Active);
    assert_eq!(original, goal());
    let mut exhausted = original.clone();
    exhausted.revision = u64::MAX;
    assert_eq!(
        update_goal(
            &exhausted,
            "goal-1",
            u64::MAX,
            Authority::RootUser,
            GoalUpdate::Pause,
            None,
            |_| false
        )
        .unwrap_err(),
        DomainError::RevisionExhausted
    );
}

/// 计划允许多并行，但只能由本会话 owner 携正确修订整表替换。
#[test]
fn plan_ownership_cas_parallel_and_dependencies() {
    let steps = vec![
        step("a", PlanStatus::InProgress),
        step("b", PlanStatus::InProgress),
    ];
    let plan = replace_plan(&Plan::default(), "root", "root", 0, steps.clone()).unwrap();
    assert_eq!(plan.steps.len(), 2);
    assert_eq!(
        replace_plan(&plan, "root", "child", 1, vec![]).unwrap_err(),
        DomainError::Unauthorized
    );
    assert_eq!(
        replace_plan(&plan, "root", "root", 0, vec![]).unwrap_err(),
        DomainError::Conflict
    );
    let cleared = replace_plan(&plan, "root", "root", 1, vec![]).unwrap();
    assert!(cleared.steps.is_empty());
    assert_eq!(cleared.revision, 2);
    let mut cycle = steps;
    cycle[0].dependencies = vec!["b".into()];
    cycle[1].dependencies = vec!["a".into()];
    assert_eq!(
        replace_plan(&plan, "root", "root", 1, cycle).unwrap_err(),
        DomainError::InvalidInput
    );
    assert!(replace_plan(
        &plan,
        "root",
        "root",
        1,
        vec![
            step("a", PlanStatus::Pending),
            step("a", PlanStatus::Pending)
        ]
    )
    .is_err());
}

#[path = "completion_tests.rs"]
mod completion_tests;

/// 阻塞记录必须覆盖最近连续三轮，等待用户或孩子不冒充阻塞证据。
#[test]
fn blocker_requires_consecutive_recent_attempts() {
    let mut goal = goal();
    let blocker = GoalBlocker {
        reason: "来源不可访问，已重复核对".into(),
        attempts: (1..=3)
            .map(|round| BlockerAttempt {
                round,
                result_refs: vec![format!("attempt-{round}")],
            })
            .collect(),
    };
    assert_eq!(
        update_goal(
            &goal,
            &goal.id,
            1,
            Authority::RootAutomatic,
            GoalUpdate::Block(blocker.clone()),
            None,
            |_| false
        )
        .unwrap_err(),
        DomainError::BlockerTooEarly { rounds_started: 0 }
    );
    goal.rounds_started = 3;
    let blocked = update_goal(
        &goal,
        &goal.id,
        1,
        Authority::RootAutomatic,
        GoalUpdate::Block(blocker.clone()),
        None,
        |_| false,
    )
    .unwrap();
    assert_eq!(blocked.phase, GoalPhase::Blocked);
    goal.rounds_started = 4;
    assert_eq!(
        update_goal(
            &goal,
            &goal.id,
            1,
            Authority::RootAutomatic,
            GoalUpdate::Block(blocker),
            None,
            |_| false
        )
        .unwrap_err(),
        DomainError::InvalidInput
    );
}
