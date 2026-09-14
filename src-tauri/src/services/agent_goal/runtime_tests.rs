use super::*;

/// 只有直接用户建立的有效目标用于运行态测试。
fn goal() -> crate::agent_autonomy_models::Goal {
    create_goal(
        None,
        Authority::RootUser,
        "goal",
        GoalSpec {
            objective: "验证".into(),
            acceptance_criteria: vec!["成功".into()],
            max_goal_rounds: 3,
        },
    )
    .unwrap()
}

/// 显式提供所有放行事实，防止默认值成为隐含许可。
fn admission() -> Admission {
    Admission {
        normal_turn_end: true,
        persisted: true,
        ..Admission::default()
    }
}

/// 恢复不武装，自动及子级无权激活；实际准入才计数，重复回调不生效。
#[test]
fn restored_runtime_and_exactly_one_admission() {
    let goal = goal();
    let input = admission();
    let mut runtime = GoalRuntime::new("execution").unwrap();
    assert!(!runtime.is_armed());
    assert_eq!(
        runtime.reserve(&goal, &input).unwrap_err(),
        DomainError::Disarmed
    );
    assert_eq!(
        runtime.arm(&goal, Authority::Child),
        Err(DomainError::Unauthorized)
    );
    assert_eq!(
        runtime.arm(&goal, Authority::RootAutomatic),
        Err(DomainError::Unauthorized)
    );
    runtime.arm(&goal, Authority::RootUser).unwrap();
    let ticket = runtime.reserve(&goal, &input).unwrap();
    assert_eq!(goal.rounds_started, 0);
    assert_eq!(
        runtime.reserve(&goal, &input).unwrap_err(),
        DomainError::Busy
    );
    let next = runtime.admit(&goal, &ticket, "execution", &input).unwrap();
    assert_eq!(next.rounds_started, 1);
    assert_eq!(next.revision, 2);
    assert_eq!(
        runtime
            .admit(&goal, &ticket, "execution", &input)
            .unwrap_err(),
        DomainError::StaleReservation
    );
    assert_eq!(
        runtime.reserve(&next, &input).unwrap_err(),
        DomainError::Busy
    );
    runtime.finish_turn(&ticket, true).unwrap();
    assert!(runtime.reserve(&next, &input).is_ok());
}

/// 两阶段之间的 revision、执行身份和用户消息变化必须重新拒绝，不增加轮数。
#[test]
fn reservation_revalidates_before_counting() {
    let goal = goal();
    let mut input = admission();
    let mut runtime = GoalRuntime::new("execution").unwrap();
    runtime.arm(&goal, Authority::RootUser).unwrap();
    let ticket = runtime.reserve(&goal, &input).unwrap();
    assert_eq!(
        runtime.admit(&goal, &ticket, "other", &input).unwrap_err(),
        DomainError::StaleReservation
    );
    let mut changed = goal.clone();
    changed.revision += 1;
    assert_eq!(
        runtime
            .admit(&changed, &ticket, "execution", &input)
            .unwrap_err(),
        DomainError::Conflict
    );
    input.human_pending = true;
    assert_eq!(
        runtime
            .admit(&goal, &ticket, "execution", &input)
            .unwrap_err(),
        DomainError::HumanPending
    );
    runtime.cancel_reservation(&ticket).unwrap();
    assert_eq!(goal.rounds_started, 0);
}

/// 等待和停止都撤销预约；迟到子通知不能重新武装根目标。
#[test]
fn waits_stops_and_late_callbacks_fail_closed() {
    let goal = goal();
    let input = admission();
    let mut runtime = GoalRuntime::new("execution").unwrap();
    runtime.arm(&goal, Authority::RootUser).unwrap();
    let old = runtime.reserve(&goal, &input).unwrap();
    runtime.wait_for_user();
    assert!(runtime.is_waiting_user());
    assert_eq!(
        runtime.reserve(&goal, &input).unwrap_err(),
        DomainError::WaitingUser
    );
    assert_eq!(
        runtime.admit(&goal, &old, "execution", &input).unwrap_err(),
        DomainError::StaleReservation
    );
    runtime.arm(&goal, Authority::RootUser).unwrap();
    let ticket = runtime.reserve(&goal, &input).unwrap();
    assert_eq!(
        runtime.cancel_reservation(&old),
        Err(DomainError::StaleReservation)
    );
    runtime.stop(StopReason::User);
    runtime.observe_goal(&goal);
    assert!(!runtime.is_armed());
    assert_eq!(runtime.stop_reason(), Some(StopReason::User));
    assert_eq!(
        runtime.reserve(&goal, &input).unwrap_err(),
        DomainError::Stopped
    );
    assert_eq!(
        runtime
            .admit(&goal, &ticket, "execution", &input)
            .unwrap_err(),
        DomainError::StaleReservation
    );
}

/// 子等待、未落盘和异常轮次都不得空转或自动重试。
#[test]
fn admission_gates_and_abnormal_turn() {
    let goal = goal();
    let ready = admission();
    let mut runtime = GoalRuntime::new("execution").unwrap();
    runtime.arm(&goal, Authority::RootUser).unwrap();
    let cases = [
        (
            Admission {
                waiting_children: true,
                ..ready.clone()
            },
            DomainError::WaitingChildren,
        ),
        (
            Admission {
                persisted: false,
                ..ready.clone()
            },
            DomainError::NotPersisted,
        ),
        (
            Admission {
                normal_turn_end: false,
                ..ready.clone()
            },
            DomainError::AbnormalTurn,
        ),
        (
            Admission {
                execution_busy: true,
                ..ready.clone()
            },
            DomainError::Busy,
        ),
    ];
    for (input, expected) in cases {
        assert_eq!(runtime.reserve(&goal, &input).unwrap_err(), expected);
    }
    let ticket = runtime.reserve(&goal, &ready).unwrap();
    let next = runtime.admit(&goal, &ticket, "execution", &ready).unwrap();
    runtime.finish_turn(&ticket, false).unwrap();
    assert_eq!(
        runtime.reserve(&next, &ready).unwrap_err(),
        DomainError::Stopped
    );
    assert_eq!(runtime.stop_reason(), Some(StopReason::AbnormalTurn));
}
