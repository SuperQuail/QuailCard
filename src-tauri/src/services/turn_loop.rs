//! turn/step 循环骨架：步进、取消检查与停滞提示的唯一实现。
//!
//! Agent 对话与卡片生成共用这个骨架；模型调用、工具执行与结束条件由各 scope
//! 通过 TurnStep 提供。循环不设轮次上限（假定有人值守），取消是唯一提前出口。

use std::future::Future;
use std::pin::Pin;

use crate::error::CommandError;

/// 各 scope 自定义的异步结果；循环不关心执行器如何构造 Future。
pub(crate) type TurnFuture<'a, T> =
    Pin<Box<dyn Future<Output = Result<T, CommandError>> + Send + 'a>>;

/// 每多少轮无进展提示一次；只提示值守者，不终止循环。
const STALL_NOTICE_ROUNDS: u32 = 3;

/// 一轮的结论，决定计数与循环是否继续。
pub(crate) enum TurnOutcome {
    /// 本轮有有效进展，停滞计数清零。
    Progress,
    /// 本轮没有进展，但循环继续。
    Stalled,
    /// 本轮正常结束。
    Finish,
}

/// 取消状态；两种任务控制各自实现，循环不感知对方的状态结构。
pub(crate) trait TurnCancel: Send + Sync {
    /// 原子读取停止标记；步内取消由各 scope 自己等待。
    fn cancel_requested(&self) -> bool;
}

/// 单步执行者：模型调用、工具处理与结束判定由各 scope 自己实现。
pub(crate) trait TurnStep: Send {
    /// 执行一轮；返回错误表示本轮无法继续，由外层决定如何保留部分结果。
    fn step<'a>(&'a mut self, step: u32) -> TurnFuture<'a, TurnOutcome>;

    /// 连续多轮无进展时的可见提示；默认无操作。
    fn stalled(&mut self, _step: u32, _rounds: u32) {}

    /// 取消收尾：Agent 报错并保留已完成操作，生成返回部分草稿后由外层收尾。
    fn cancel(&mut self) -> Result<(), CommandError>;
}

/// 唯一的 turn/step 骨架；取消是唯一提前出口，不设轮次上限。
pub(crate) async fn drive<S, C>(step: &mut S, cancel: &C) -> Result<(), CommandError>
where
    S: TurnStep,
    C: TurnCancel,
{
    let mut index = 0u32;
    let mut stagnant = 0u32;
    loop {
        if cancel.cancel_requested() {
            return step.cancel();
        }
        // 长任务的展示计数饱和，不把整数溢出变成隐式执行上限。
        index = index.saturating_add(1);
        match step.step(index).await? {
            TurnOutcome::Progress => stagnant = 0,
            TurnOutcome::Stalled => {
                stagnant = stagnant.saturating_add(1);
                if stagnant.is_multiple_of(STALL_NOTICE_ROUNDS) {
                    step.stalled(index, stagnant);
                }
            }
            TurnOutcome::Finish => return Ok(()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, Mutex};

    struct FakeCancel(bool);
    impl TurnCancel for FakeCancel {
        /// 固定标记足以覆盖循环入口与取消收尾两条路径。
        fn cancel_requested(&self) -> bool {
            self.0
        }
    }

    #[derive(Default)]
    struct Trace {
        steps: Vec<u32>,
        stalls: Vec<u32>,
        cancelled: bool,
    }

    struct FakeStep {
        outcomes: Vec<TurnOutcome>,
        trace: Arc<Mutex<Trace>>,
    }

    impl TurnStep for FakeStep {
        /// 按脚本逐轮返回结论，并记录实际轮次。
        fn step<'a>(&'a mut self, step: u32) -> TurnFuture<'a, TurnOutcome> {
            Box::pin(async move {
                self.trace.lock().unwrap().steps.push(step);
                Ok(self.outcomes.remove(0))
            })
        }

        fn stalled(&mut self, _step: u32, rounds: u32) {
            self.trace.lock().unwrap().stalls.push(rounds);
        }

        fn cancel(&mut self) -> Result<(), CommandError> {
            self.trace.lock().unwrap().cancelled = true;
            Ok(())
        }
    }

    #[tokio::test]
    /// 轮次从 1 递增，Finish 立即结束循环。
    async fn advances_steps_until_finish() {
        let trace = Arc::new(Mutex::new(Trace::default()));
        let mut step = FakeStep {
            outcomes: vec![TurnOutcome::Progress, TurnOutcome::Finish],
            trace: trace.clone(),
        };
        drive(&mut step, &FakeCancel(false)).await.unwrap();
        assert_eq!(trace.lock().unwrap().steps, vec![1, 2]);
    }

    #[tokio::test]
    /// 无进展按固定间隔提示且不终止；有进展会清零计数。
    async fn stall_notice_is_periodic_and_reset_by_progress() {
        let trace = Arc::new(Mutex::new(Trace::default()));
        let mut step = FakeStep {
            outcomes: vec![
                TurnOutcome::Stalled,
                TurnOutcome::Stalled,
                TurnOutcome::Progress,
                TurnOutcome::Stalled,
                TurnOutcome::Stalled,
                TurnOutcome::Stalled,
                TurnOutcome::Finish,
            ],
            trace: trace.clone(),
        };
        drive(&mut step, &FakeCancel(false)).await.unwrap();
        assert_eq!(trace.lock().unwrap().stalls, vec![3]);
        assert_eq!(trace.lock().unwrap().steps.len(), 7);
    }

    #[tokio::test]
    /// 取消在进入下一轮之前收尾，且不再请求模型。
    async fn cancel_stops_before_next_step() {
        let trace = Arc::new(Mutex::new(Trace::default()));
        let mut step = FakeStep {
            outcomes: vec![],
            trace: trace.clone(),
        };
        drive(&mut step, &FakeCancel(true)).await.unwrap();
        let trace = trace.lock().unwrap();
        assert!(trace.cancelled);
        assert!(trace.steps.is_empty());
    }
}
