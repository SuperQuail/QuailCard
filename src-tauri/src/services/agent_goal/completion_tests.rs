use super::*;

/// 使用实际收据格式，验证器是否可信由宿主组合测试另行覆盖。
fn evidence() -> Vec<GoalEvidence> {
    vec![GoalEvidence {
        criterion_index: 0,
        goal_revision: 1,
        source_version: "v1".into(),
        receipt_ref: "tool:saved-note".into(),
    }]
}

/// 计划来源仅为追踪资料，不应把任意旧引用变成无法满足的完成前置。
fn completed_plan() -> Plan {
    let mut row = step("a", PlanStatus::Completed);
    row.result_refs = vec![
        "task:old".into(),
        "subagent:old".into(),
        "change:old".into(),
        "legacy".into(),
    ];
    replace_plan(&Plan::default(), "root", "root", 0, vec![row]).unwrap()
}

/// 拒绝要能区分资料、版本与条件问题，不再要求历史 resultRefs 全部变成 evidence。
#[test]
fn completion_evidence_diagnostics_and_coverage() {
    let goal = goal();
    let plan = completed_plan();
    let evidence = evidence();
    let mut ctx = context(&plan);
    assert_eq!(
        check_completion(&goal, &evidence, &ctx, |_| false),
        Err(DomainError::Completion(CompletionIssue::EvidenceReceipt(0)))
    );
    assert_eq!(
        check_completion(&goal, &[], &ctx, |_| true),
        Err(DomainError::Completion(CompletionIssue::EvidenceCount))
    );
    assert_eq!(
        check_completion(&goal, &vec![evidence[0].clone(); 257], &ctx, |_| true),
        Err(DomainError::Completion(CompletionIssue::EvidenceCount))
    );
    ctx.source_version = "v2";
    assert_eq!(
        check_completion(&goal, &evidence, &ctx, |_| true),
        Err(DomainError::Completion(CompletionIssue::EvidenceSource(0)))
    );
    ctx.source_version = "v1";
    let mut bad = evidence.clone();
    bad[0].goal_revision = 0;
    assert_eq!(
        check_completion(&goal, &bad, &ctx, |_| true),
        Err(DomainError::Completion(CompletionIssue::EvidenceRevision(
            0
        )))
    );
    bad = evidence.clone();
    bad[0].criterion_index = 1;
    assert_eq!(
        check_completion(&goal, &bad, &ctx, |_| true),
        Err(DomainError::Completion(CompletionIssue::EvidenceCriterion(
            0
        )))
    );
    bad = evidence.clone();
    bad[0].receipt_ref.clear();
    assert_eq!(
        check_completion(&goal, &bad, &ctx, |_| true),
        Err(DomainError::Completion(CompletionIssue::EvidenceReceipt(0)))
    );
    let mut multi = goal.clone();
    multi.acceptance_criteria.push("内容有依据".into());
    assert_eq!(
        check_completion(&multi, &evidence, &ctx, |_| true),
        Err(DomainError::Completion(CompletionIssue::MissingCriterion(
            1
        )))
    );
    let mut shared = evidence.clone();
    shared.push(GoalEvidence {
        criterion_index: 1,
        ..evidence[0].clone()
    });
    assert!(check_completion(&multi, &shared, &ctx, |_| true).is_ok());
    let complete = update_goal(
        &goal,
        &goal.id,
        1,
        Authority::RootAutomatic,
        GoalUpdate::Complete(evidence),
        Some(&ctx),
        |_| true,
    )
    .unwrap();
    assert_eq!(complete.phase, GoalPhase::Complete);
    assert_eq!(complete.revision, 2);
    assert_eq!(goal.phase, GoalPhase::Active);
    assert_eq!(
        update_goal(
            &complete,
            &goal.id,
            2,
            Authority::RootUser,
            GoalUpdate::Resume,
            None,
            |_| false
        ),
        Err(DomainError::InvalidPhase)
    );
}

/// 未收尾的真实工作仍拒绝；可选步骤不会放宽证据覆盖或子树收尾要求。
#[test]
fn completion_checks_required_steps_and_settlement() {
    let goal = goal();
    let mut plan = completed_plan();
    let evidence = evidence();
    let mut ctx = context(&plan);
    ctx.owner_session_id = "other";
    assert_eq!(
        check_completion(&goal, &evidence, &ctx, |_| true),
        Err(DomainError::Completion(CompletionIssue::PlanOwner))
    );
    ctx.owner_session_id = "root";
    ctx.pending_messages = 1;
    assert_eq!(
        check_completion(&goal, &evidence, &ctx, |_| true),
        Err(DomainError::Completion(CompletionIssue::PendingWork))
    );
    ctx.pending_messages = 0;
    ctx.pending_children = 1;
    assert_eq!(
        check_completion(&goal, &evidence, &ctx, |_| true),
        Err(DomainError::Completion(CompletionIssue::PendingWork))
    );
    ctx.pending_children = 0;
    ctx.descendants_settled = false;
    assert_eq!(
        check_completion(&goal, &evidence, &ctx, |_| true),
        Err(DomainError::Completion(CompletionIssue::PendingWork))
    );
    for status in [
        PlanStatus::Pending,
        PlanStatus::InProgress,
        PlanStatus::Blocked,
        PlanStatus::Cancelled,
    ] {
        plan.steps[0].status = status;
        assert_eq!(
            check_completion(&goal, &evidence, &context(&plan), |_| true),
            Err(DomainError::Completion(CompletionIssue::RequiredStep(0)))
        );
    }
    plan.steps[0].required = false;
    assert!(check_completion(&goal, &evidence, &context(&plan), |_| true).is_ok());
    assert_eq!(
        check_completion(&goal, &[], &context(&plan), |_| true),
        Err(DomainError::Completion(CompletionIssue::EvidenceCount))
    );
}
