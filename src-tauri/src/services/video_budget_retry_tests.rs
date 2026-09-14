//! 补充重试终态与直接 Drop 的生命周期契约。
use super::*;

#[tokio::test(start_paused = true)]
/// 已耗尽请求的最后一个 429 仍影响其它任务，不能随逻辑请求结束清除冷却。
async fn final_rate_limit_still_cools_other_tasks() {
    let budget = VideoBudget::default();
    let control = control("first");
    let fake = Fake::failing("PROVIDER_RATE_LIMITED", 3, Some(1000));
    let model = BudgetedModel {
        inner: &fake,
        budget: &budget,
        provider_id: "p",
        control: &control,
    };
    assert_eq!(
        model
            .call("", &[], &[], &sink, &sink)
            .await
            .err()
            .unwrap()
            .code,
        "PROVIDER_RATE_LIMITED"
    );
    assert_eq!(fake.calls.load(Ordering::SeqCst), 3);
    assert_eq!(budget.inner.state().active, 0);
    let until = budget.inner.state().providers["p"].until.unwrap();
    assert!(until >= Instant::now() + Duration::from_millis(1000));
    let mut next = Box::pin(budget.model("p", "second", &control));
    assert!(poll!(&mut next).is_pending());
    drop(next);
    assert!(budget.inner.state().waiting.is_empty());
}

#[tokio::test(start_paused = true)]
/// 已有较长服务端等待提示不能被迟到的短等待覆盖。
async fn shared_cooldown_only_extends() {
    let budget = VideoBudget::default();
    budget.cool_down("p", Duration::from_secs(10));
    let until = budget.inner.state().providers["p"].until;
    budget.cool_down("p", Duration::from_secs(1));
    assert_eq!(budget.inner.state().providers["p"].until, until);
    budget.cool_down("p", Duration::from_secs(20));
    assert!(budget.inner.state().providers["p"].until > until);
}

#[tokio::test(start_paused = true)]
/// 上层失败后直接丢弃 sibling future 也能回收流，不要求先显式设置取消标志。
async fn dropping_active_call_returns_every_model_slot() {
    let budget = VideoBudget::default();
    let control = control("task");
    let fake = Fake {
        delay: Duration::from_secs(3600),
        ..Default::default()
    };
    let model = BudgetedModel {
        inner: &fake,
        budget: &budget,
        provider_id: "p",
        control: &control,
    };
    let mut call = model.call("", &[], &[], &sink, &sink);
    assert!(poll!(&mut call).is_pending());
    assert_eq!(budget.inner.state().active, 1);
    drop(call);
    assert_eq!(fake.active.load(Ordering::SeqCst), 0);
    assert_eq!(budget.inner.state().active, 0);
    assert_eq!(budget.inner.state().providers["p"].active, 0);
    assert!(budget.inner.state().waiting.is_empty());
}

#[tokio::test(start_paused = true)]
/// 429 的过载正文沿用 OVERLOADED 分类时，即使没有头提示也必须共享退避。
async fn overloaded_projection_also_shares_cooldown() {
    let budget = VideoBudget::default();
    let control = control("first");
    let fake = Fake::failing("PROVIDER_OVERLOADED", 1, None);
    let model = BudgetedModel {
        inner: &fake,
        budget: &budget,
        provider_id: "p",
        control: &control,
    };
    let mut call = model.call("", &[], &[], &sink, &sink);
    assert!(poll!(&mut call).is_pending());
    assert!(budget.inner.state().providers["p"].until.unwrap() > Instant::now());
    assert_eq!(budget.inner.state().active, 0);
    control.cancel();
    assert_eq!(call.await.err().unwrap().code, "VIDEO_CANCELLED");
}
