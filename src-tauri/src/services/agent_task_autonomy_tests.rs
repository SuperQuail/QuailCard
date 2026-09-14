use super::*;

#[test]
/// 最终状态由根控制器收尾发布，不把 active Goal 的模型轮次结束标为成功。
fn distinguishes_goal_completion_from_turn_end() {
    for (phase, waiting, expected) in [
        ("active", None, "paused"),
        ("active", Some("waitingUser"), "waiting"),
        ("blocked", None, "blocked"),
        ("complete", None, "completed"),
        ("", None, "completed"),
    ] {
        let tasks = AgentTasks::default();
        let (control, _) = tasks.register("main", "root", "run", "session").unwrap();
        control.update(|state| { state.goal_phase = phase.into(); state.waiting_reason = waiting.map(String::from); });
        control.complete(None);
        assert_eq!(control.snapshot().state, expected);
        control.update(|state| state.state = "running".into());
        assert_eq!(control.snapshot().state, expected);
    }
}

#[tokio::test]
/// 取消广播唤醒所有已登记等待者，迟到等待者也通过原子停止标记直接返回。
async fn cancellation_wakes_all_waiters() {
    let tasks = AgentTasks::default();
    let (control, _) = tasks.register("main", "root", "run", "session").unwrap();
    let first = control.cancelled();
    let second = control.cancelled();
    let all = async {
        tokio::join!(first, second, async { tokio::task::yield_now().await; control.cancel(); });
        control.cancelled().await;
    };
    tokio::time::timeout(std::time::Duration::from_secs(1), all).await.unwrap();
    control.complete(None);
    assert_eq!(control.snapshot().state, "cancelled");
}
