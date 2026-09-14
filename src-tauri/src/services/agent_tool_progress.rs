//! 异步工具等待期间只转发适配器的安全进度，不读取视频结果正文或写入历史。
use crate::{
    error::CommandError,
    services::{agent_ports::AgentVideo, agent_tasks::AgentControl},
};
use std::{future::Future, time::Duration};

/// 复用既有取消语义；阶段快照仅在变化时更新，工具结束后不遗留旧识别进度。
pub(super) async fn wait<T>(
    operation: impl Future<Output = Result<T, CommandError>>,
    video: &dyn AgentVideo,
    control: &AgentControl,
    label: &str,
) -> Result<T, CommandError> {
    tokio::pin!(operation);
    let mut interval = tokio::time::interval(Duration::from_millis(500));
    interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    let mut previous = None;
    let result = loop {
        tokio::select! {
            biased;
            _ = control.cancelled() => break Err(CommandError::new("AGENT_CANCELLED", "已停止")),
            result = &mut operation => break result,
            _ = interval.tick() => {
                let next = video.progress();
                if next != previous {
                    control.update(|state| state.phase = next.as_deref().unwrap_or(label).to_string());
                    previous = next;
                }
            }
        }
    };
    if previous.is_some() && !control.is_cancelled() {
        control.update(|state| state.phase = label.to_string());
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::services::{agent_ports::AgentFuture, agent_tasks::AgentTasks};
    use serde_json::Value;
    use std::sync::Mutex;
    struct Video(Mutex<Option<String>>);
    impl AgentVideo for Video {
        /// 进度测试不启动任何真实视频任务。
        fn run<'a>(&'a self, _: &'a str, _: bool) -> AgentFuture<'a, Value> {
            Box::pin(async { Ok(Value::Null) })
        }
        /// 模拟可变化但不含原始字幕或凭据的安全阶段。
        fn progress(&self) -> Option<String> {
            self.0.lock().unwrap().clone()
        }
    }
    /// 实时快照可见处理量，返回后清除旧阶段，不产生持久消息。
    #[tokio::test(start_paused = true)]
    async fn forwards_progress_and_restores_phase_after_result() {
        let tasks = AgentTasks::default();
        let (control, _) = tasks.register("owner", "root", "run", "session").unwrap();
        let video = Video(Mutex::new(Some(
            "Whisper 50% · 01:00 / 02:00 · 2.0×".into(),
        )));
        let work = async {
            tokio::time::sleep(Duration::from_millis(510)).await;
            assert!(control.snapshot().phase.contains("Whisper 50%"));
            *video.0.lock().unwrap() = Some("Whisper 60%".into());
            tokio::time::sleep(Duration::from_millis(510)).await;
            assert_eq!(control.snapshot().phase, "Whisper 60%");
            Ok(7)
        };
        assert_eq!(wait(work, &video, &control, "视频转录").await.unwrap(), 7);
        assert_eq!(control.snapshot().phase, "视频转录");
    }
    /// 取消不能被进度轮询吞掉，也不等待尚未完成的工具。
    #[tokio::test(start_paused = true)]
    async fn cancellation_still_stops_waiting_tool() {
        let tasks = AgentTasks::default();
        let (control, _) = tasks.register("owner", "root", "run", "session").unwrap();
        let video = Video(Mutex::new(None));
        let work = async {
            tokio::time::sleep(Duration::from_millis(600)).await;
            control.cancel();
            std::future::pending::<Result<(), CommandError>>().await
        };
        assert_eq!(
            wait(work, &video, &control, "视频转录")
                .await
                .unwrap_err()
                .code,
            "AGENT_CANCELLED"
        );
    }
}
