use super::*;

#[tokio::test]
/// 连续空响应超过旧阈值仍继续，直到用户取消才收尾。
async fn empty_rounds_continue_until_user_cancels() {
    let control = GenerationControl::new("manual-stop".into());
    let mut steps = (0..12).map(|_| batch(vec![])).collect::<Vec<_>>();
    steps.push(Step::Cancel(control.clone()));
    let model = scripted_model(steps);
    let result = run(&model, &input(), &control).await.unwrap();
    assert_eq!(model.calls.load(Ordering::SeqCst), 13);
    assert!(control.is_cancelled());
    assert!(result.cards.is_empty());
    assert!(result
        .warnings
        .iter()
        .any(|warning| warning.contains("已停止")));
}

#[tokio::test]
/// 重复落卡不强制终止，用户停止时保留之前的有效草稿。
async fn duplicate_rounds_preserve_drafts_until_user_cancels() {
    let control = GenerationControl::new("manual-stop-drafts".into());
    let mut steps = vec![batch(vec![
        plan(&["考点"]),
        call("emit_card", arguments("问题")),
    ])];
    steps.extend((0..12).map(|_| batch(vec![call("emit_card", arguments("问题"))])));
    steps.push(Step::Cancel(control.clone()));
    let model = scripted_model(steps);
    let result = run(&model, &input(), &control).await.unwrap();
    assert_eq!(model.calls.load(Ordering::SeqCst), 14);
    assert!(control.is_cancelled());
    assert_eq!(result.cards.len(), 1);
}
