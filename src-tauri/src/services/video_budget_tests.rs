//! 纯 fake 验证预算，不访问网络或媒体，也不依赖真实退避时长。
use super::*;
#[path = "video_budget_retry_tests.rs"]
mod retry_tests;
use crate::{
    ai::ToolDefinition,
    services::{
        agent_ports::{AgentFuture, AgentModel, AgentModelReply},
        video_tasks::VideoTaskRegistry,
    },
    storage::video::VideoTaskRecord,
};
use futures_util::{future::join_all, poll};
use serde_json::Value;
use std::sync::atomic::{AtomicUsize, Ordering};

/// 为测试创建真实控制句柄，注册表退出不影响句柄的取消状态。
fn control(id: &str) -> VideoControl {
    VideoTaskRegistry::default()
        .register(id, &VideoTaskRecord::new(id, "video", "source"))
        .unwrap()
}

#[derive(Default)]
struct Fake {
    calls: AtomicUsize,
    active: AtomicUsize,
    peak: AtomicUsize,
    failures: Mutex<VecDeque<CommandError>>,
    delay: Duration,
    traces: Mutex<Vec<String>>,
}

struct Active<'a>(&'a AtomicUsize);
impl Drop for Active<'_> {
    /// 模拟 HTTP future 被丢弃即关闭流，不允许取消留下活跃请求。
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::SeqCst);
    }
}

impl Fake {
    /// 注入固定安全错误序列，耗尽后成功，便于统计真实 attempt 数。
    fn failing(code: &'static str, count: usize, retry_after: Option<u64>) -> Self {
        Self {
            failures: Mutex::new(
                (0..count)
                    .map(|_| CommandError::new(code, "safe").with_retry_after(retry_after))
                    .collect(),
            ),
            ..Default::default()
        }
    }
}

impl AgentModel for Fake {
    /// 整个模拟流持有活跃计数，测试可观察到真实重叠峰值。
    fn call<'a>(
        &'a self,
        _system: &'a str,
        _messages: &'a [Value],
        _tools: &'a [ToolDefinition],
        _delta: &'a (dyn Fn(&str) + Send + Sync),
        _reasoning: &'a (dyn Fn(&str) + Send + Sync),
    ) -> AgentFuture<'a, AgentModelReply> {
        Box::pin(async move {
            self.calls.fetch_add(1, Ordering::SeqCst);
            let active = self.active.fetch_add(1, Ordering::SeqCst) + 1;
            self.peak.fetch_max(active, Ordering::SeqCst);
            let _active = Active(&self.active);
            if !self.delay.is_zero() {
                tokio::time::sleep(self.delay).await;
            }
            let error = self.failures.lock().unwrap().pop_front();
            match error {
                Some(error) => Err(error),
                None => Ok(AgentModelReply::default()),
            }
        })
    }

    /// 记录追踪身份以验证包装器没有丢失调用方关联。
    fn call_traced<'a>(
        &'a self,
        system: &'a str,
        messages: &'a [Value],
        tools: &'a [ToolDefinition],
        delta: &'a (dyn Fn(&str) + Send + Sync),
        reasoning: &'a (dyn Fn(&str) + Send + Sync),
        trace: &'a str,
    ) -> AgentFuture<'a, AgentModelReply> {
        self.traces.lock().unwrap().push(trace.to_string());
        self.call(system, messages, tools, delta, reasoning)
    }
}

/// 忽略展示增量，断言只针对预算与传输生命周期。
fn sink(_: &str) {}

#[tokio::test(start_paused = true)]
/// 克隆预算在多任务同供应商下共享三个槽，真实流峰值大于一且不超过三。
async fn fake_model_peak_is_shared_across_tasks() {
    let budget = VideoBudget::default();
    let clone = budget.clone();
    let fake = Fake {
        delay: Duration::from_millis(10),
        ..Default::default()
    };
    let controls: Vec<_> = (0..12).map(|i| control(&format!("task-{i}"))).collect();
    let models: Vec<_> = controls
        .iter()
        .enumerate()
        .map(|(i, control)| BudgetedModel {
            inner: &fake,
            budget: if i % 2 == 0 { &budget } else { &clone },
            provider_id: "shared",
            control,
        })
        .collect();
    let results = join_all(
        models
            .iter()
            .map(|model| model.call("", &[], &[], &sink, &sink)),
    )
    .await;
    assert!(results.iter().all(Result::is_ok));
    assert_eq!(fake.peak.load(Ordering::SeqCst), 3);
    assert_eq!(fake.calls.load(Ordering::SeqCst), 12);
    assert_eq!(budget.inner.state().active, 0);
    assert_eq!(budget.inner.state().providers["shared"].active, 0);
}

#[tokio::test(start_paused = true)]
/// 不同供应商仍受视频全局三个槽约束，不能分别创建三份额度。
async fn global_peak_spans_providers() {
    let budget = VideoBudget::default();
    let fake = Fake {
        delay: Duration::from_millis(10),
        ..Default::default()
    };
    let control = control("task");
    let models: Vec<_> = ["a", "b", "c", "d", "e", "f"]
        .iter()
        .map(|provider_id| BudgetedModel {
            inner: &fake,
            budget: &budget,
            provider_id,
            control: &control,
        })
        .collect();
    let results = join_all(
        models
            .iter()
            .map(|model| model.call("", &[], &[], &sink, &sink)),
    )
    .await;
    assert!(results.iter().all(Result::is_ok));
    assert_eq!(fake.peak.load(Ordering::SeqCst), 3);
}

#[tokio::test(start_paused = true)]
/// 无论重试能否成功，单次逻辑调用最多发送三次，并原样透传 trace。
async fn retry_stops_after_three_attempts() {
    let budget = VideoBudget::default();
    let control = control("task");
    for (count, success) in [(2, true), (5, false)] {
        let fake = Fake::failing("PROVIDER_TIMEOUT", count, None);
        let model = BudgetedModel {
            inner: &fake,
            budget: &budget,
            provider_id: "provider",
            control: &control,
        };
        let result = model
            .call_traced("", &[], &[], &sink, &sink, "trace-id")
            .await;
        assert_eq!(result.is_ok(), success);
        assert_eq!(fake.calls.load(Ordering::SeqCst), 3);
        assert_eq!(*fake.traces.lock().unwrap(), vec!["trace-id"; 3]);
        assert_eq!(budget.inner.state().active, 0);
    }
}

#[tokio::test(start_paused = true)]
/// 401 类认证及结构错误立即失败，不消耗后续传输 attempt。
async fn authentication_and_invalid_reply_are_not_retried() {
    let budget = VideoBudget::default();
    let control = control("task");
    for code in [
        "PROVIDER_AUTH_FAILED",
        "PROVIDER_RESPONSE_INVALID",
        "PROVIDER_BAD_REQUEST",
        "PROVIDER_QUOTA_EXCEEDED",
    ] {
        let fake = Fake::failing(code, 5, None);
        let model = BudgetedModel {
            inner: &fake,
            budget: &budget,
            provider_id: "provider",
            control: &control,
        };
        assert_eq!(
            model
                .call("", &[], &[], &sink, &sink)
                .await
                .err()
                .unwrap()
                .code,
            code
        );
        assert_eq!(fake.calls.load(Ordering::SeqCst), 1);
    }
}

#[tokio::test(start_paused = true)]
/// 429 冷却被另一个任务看见，但不会占住其它供应商的全局额度。
async fn rate_limit_is_shared_and_does_not_reserve_global_slot() {
    let budget = VideoBudget::default();
    let first = control("first");
    let second = control("second");
    let fake = Fake::failing("PROVIDER_RATE_LIMITED", 1, Some(1000));
    let model = BudgetedModel {
        inner: &fake,
        budget: &budget,
        provider_id: "cool",
        control: &first,
    };
    let call = model.call("", &[], &[], &sink, &sink);
    tokio::pin!(call);
    assert!(poll!(&mut call).is_pending());
    assert_eq!(budget.inner.state().active, 0);
    let waiting = budget.model("cool", "second", &second);
    tokio::pin!(waiting);
    assert!(poll!(&mut waiting).is_pending());
    let ready = budget.model("ready", "second", &second).await.unwrap();
    assert_eq!(budget.inner.state().active, 1);
    drop(ready);
    tokio::time::advance(Duration::from_millis(999)).await;
    assert!(poll!(&mut waiting).is_pending());
    assert_eq!(fake.calls.load(Ordering::SeqCst), 1);
    first.cancel();
    assert_eq!(call.await.err().unwrap().code, "VIDEO_CANCELLED");
    // 取消施加冷却的任务不能清除同供应商已发布的冷却。
    assert!(poll!(&mut waiting).is_pending());
    drop(waiting.await.unwrap());
    assert_eq!(budget.inner.state().active, 0);
}

#[tokio::test(start_paused = true)]
/// 同任务连续排队不能饿死其它就绪任务，按任务而非请求数量轮转。
async fn model_queue_round_robins_tasks() {
    let budget = VideoBudget::default();
    let control = control("control");
    let mut held = Vec::new();
    for _ in 0..3 {
        held.push(budget.model("p", "hold", &control).await.unwrap());
    }
    let a1 = budget.model("p", "a", &control);
    let a2 = budget.model("p", "a", &control);
    let b = budget.model("p", "b", &control);
    let c = budget.model("p", "c", &control);
    tokio::pin!(a1, a2, b, c);
    assert!(poll!(&mut a1).is_pending());
    assert!(poll!(&mut a2).is_pending());
    assert!(poll!(&mut b).is_pending());
    assert!(poll!(&mut c).is_pending());
    drop(held.pop());
    let first = a1.await.unwrap();
    assert!(poll!(&mut a2).is_pending());
    drop(first);
    let second = b.await.unwrap();
    assert!(poll!(&mut a2).is_pending());
    drop(second);
    drop(c.await.unwrap());
    drop(a2.await.unwrap());
    drop(held);
    assert_eq!(budget.inner.state().active, 0);
}

#[tokio::test(start_paused = true)]
/// 排队 future 直接丢弃以及显式取消都清理队列，不妨碍后续任务。
async fn dropped_and_cancelled_waiters_leave_no_reservation() {
    let budget = VideoBudget::default();
    let control = control("task");
    let mut held = Vec::new();
    for _ in 0..3 {
        held.push(budget.model("p", "hold", &control).await.unwrap());
    }
    let mut abandoned = Box::pin(budget.model("p", "abandoned", &control));
    assert!(poll!(&mut abandoned).is_pending());
    drop(abandoned);
    assert!(budget.inner.state().waiting.is_empty());
    let mut waiting = Box::pin(budget.model("p", "cancel", &control));
    assert!(poll!(&mut waiting).is_pending());
    control.cancel();
    assert_eq!(waiting.await.err().unwrap().code, "VIDEO_CANCELLED");
    assert!(budget.inner.state().waiting.is_empty());
    drop(held);
    assert_eq!(budget.inner.state().active, 0);
}

#[tokio::test(start_paused = true)]
/// 活跃流被取消后立即归还全局与供应商许可，底层 future 同时销毁。
async fn cancelling_active_stream_returns_permit() {
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
    control.cancel();
    assert_eq!(call.await.err().unwrap().code, "VIDEO_CANCELLED");
    assert_eq!(fake.active.load(Ordering::SeqCst), 0);
    assert_eq!(budget.inner.state().active, 0);
}

#[tokio::test(start_paused = true)]
/// 各媒体阶段具有独立额度，等待可取消且 Drop 归还所有许可。
async fn media_permits_have_fixed_limits_and_cancel_cleanly() {
    let budget = VideoBudget::default();
    let control = control("task");
    let frames = vec![
        budget.frame(&control).await.unwrap(),
        budget.frame(&control).await.unwrap(),
    ];
    let download = budget.download(&control).await.unwrap();
    let images = vec![
        budget.images(&control).await.unwrap(),
        budget.images(&control).await.unwrap(),
        budget.images(&control).await.unwrap(),
    ];
    let frame_wait = budget.frame(&control);
    let download_wait = budget.download(&control);
    let images_wait = budget.images(&control);
    tokio::pin!(frame_wait, download_wait, images_wait);
    assert!(poll!(&mut frame_wait).is_pending());
    assert!(poll!(&mut download_wait).is_pending());
    assert!(poll!(&mut images_wait).is_pending());
    control.cancel();
    assert_eq!(frame_wait.await.err().unwrap().code, "VIDEO_CANCELLED");
    assert_eq!(download_wait.await.err().unwrap().code, "VIDEO_CANCELLED");
    assert_eq!(images_wait.await.err().unwrap().code, "VIDEO_CANCELLED");
    drop((frames, download, images));
    assert_eq!(budget.inner.frame.available_permits(), 2);
    assert_eq!(budget.inner.download.available_permits(), 1);
    assert_eq!(budget.inner.images.available_permits(), 3);
}
