use super::*;
use crate::agent_autonomy_models::{BlockerAttempt, Goal, GoalBlocker, GoalPhase};

/// 旧文件无论缺省、零、超过旧硬限或达到整数边界，都保留原 wire 类型。
#[test]
fn legacy_round_limits_round_trip_and_never_reject_lifecycle() {
    for legacy_limit in [None, Some(0), Some(1), Some(16), Some(257), Some(u32::MAX)] {
        let mut stored = serde_json::json!({
            "id": "goal", "revision": 1, "objective": "验证兼容",
            "acceptanceCriteria": ["已核对"], "phase": "blocked", "roundsStarted": u32::MAX
        });
        if let Some(limit) = legacy_limit {
            stored["maxGoalRounds"] = limit.into();
        }
        let original: Goal = serde_json::from_value(stored).unwrap();
        let expected = legacy_limit.unwrap_or(0);
        assert_eq!(original.max_goal_rounds, expected);
        assert_eq!(
            serde_json::to_value(&original).unwrap()["maxGoalRounds"],
            expected
        );
        let resumed = update_goal(
            &original,
            "goal",
            1,
            Authority::RootUser,
            GoalUpdate::Resume,
            None,
            |_| false,
        )
        .unwrap();
        assert_eq!(resumed.phase, GoalPhase::Active);
        assert_eq!(resumed.max_goal_rounds, expected);
        let spec = GoalSpec {
            objective: "编辑目标".into(),
            acceptance_criteria: vec!["已核对".into()],
            max_goal_rounds: expected,
        };
        let created = create_goal(None, Authority::RootUser, "new", spec.clone()).unwrap();
        assert_eq!(created.max_goal_rounds, expected);
        let edited = update_goal(
            &resumed,
            "goal",
            resumed.revision,
            Authority::RootUser,
            GoalUpdate::Edit(spec),
            None,
            |_| false,
        )
        .unwrap();
        assert_eq!(edited.objective, "编辑目标");
        assert_eq!(edited.rounds_started, u32::MAX);
    }
}

/// 已耗尽旧额度的目标仍可武装并连续越过旧硬限；只有真实准入计数。
#[test]
fn automatic_rounds_continue_past_every_legacy_limit() {
    for legacy_limit in [0, 1, 16, 256, u32::MAX] {
        let mut goal = Goal {
            id: "goal".into(),
            revision: 1,
            objective: "验证".into(),
            acceptance_criteria: vec!["成功".into()],
            phase: GoalPhase::Active,
            max_goal_rounds: legacy_limit,
            ..Goal::default()
        };
        let mut runtime = GoalRuntime::new("execution").unwrap();
        runtime.arm(&goal, Authority::RootUser).unwrap();
        let admission = Admission {
            normal_turn_end: true,
            persisted: true,
            ..Admission::default()
        };
        for round in 1..=300 {
            let ticket = runtime.reserve(&goal, &admission).unwrap();
            let next = runtime
                .admit(&goal, &ticket, "execution", &admission)
                .unwrap();
            assert_eq!(next.rounds_started, round);
            assert_eq!(next.max_goal_rounds, legacy_limit);
            runtime.finish_turn(&ticket, true).unwrap();
            goal = next;
        }
        goal.rounds_started = u32::MAX - 1;
        for _ in 0..3 {
            let ticket = runtime.reserve(&goal, &admission).unwrap();
            let next = runtime
                .admit(&goal, &ticket, "execution", &admission)
                .unwrap();
            assert_eq!(next.rounds_started, u32::MAX);
            assert_eq!(next.revision, goal.revision + 1);
            runtime.finish_turn(&ticket, true).unwrap();
            goal = next;
        }
        runtime.arm(&goal, Authority::RootUser).unwrap();
    }
}

/// 阻塞证明只限制载荷条数；最大展示轮数的最近三轮计算不得溢出。
#[test]
fn blocker_attempts_remain_valid_at_saturated_round_count() {
    let goal = Goal {
        rounds_started: u32::MAX,
        max_goal_rounds: 1,
        ..Goal::default()
    };
    let blocker = GoalBlocker {
        reason: "来源仍不可访问".into(),
        attempts: ((u32::MAX - 2)..=u32::MAX)
            .map(|round| BlockerAttempt {
                round,
                result_refs: vec!["tool:attempt".into()],
            })
            .collect(),
    };
    assert_eq!(validate_blocker(&goal, &blocker), Ok(()));
}
