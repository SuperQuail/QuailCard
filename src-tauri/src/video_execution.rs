//! 视频执行的所有权边界：调用者退出只请求取消，执行者负责收尾与异常兜底。
use std::future::Future;

use crate::{
    error::CommandError,
    services::video_tasks::VideoControl,
    storage::{now_timestamp, video::VideoStorage},
    video::models::VideoTaskStatus,
};

/// 每次登记仅有一个执行守卫；不能把终结逻辑挂到可克隆的 VideoControl 上。
pub(super) struct Execution {
    control: VideoControl,
    storage: VideoStorage,
    finished: bool,
}

impl Execution {
    /// 登记成功后立即创建，覆盖尚未启动、正常运行和异常丢弃的整个生命周期。
    pub(super) fn new(control: VideoControl, storage: VideoStorage) -> Self {
        Self {
            control,
            storage,
            finished: false,
        }
    }

    /// 流水线已清理并落盘后才释放运行槽；落盘失败也不能遗留内存占用。
    pub(super) fn finish(&mut self, result: &Result<(), CommandError>) {
        if self.finished {
            return;
        }
        if let Err(error) = result {
            self.persist_failure(error);
        }
        self.control.complete(result);
        self.finished = true;
    }

    /// 仅补齐未终结记录，保留已写笔记、转录和既有终态；错误只记后端日志。
    fn persist_failure(&self, error: &CommandError) {
        let status = self.control.snapshot();
        let save = || -> Result<(), CommandError> {
            if let Some(mut record) = self.storage.load_task(&status.task_id)? {
                if matches!(record.state.as_str(), "queued" | "running") {
                    record.state = if error.code == "VIDEO_CANCELLED" {
                        "cancelled"
                    } else {
                        "failed"
                    }
                    .to_string();
                    record.step = status.step;
                    record.progress = status.progress;
                    record.error = Some(error.message.clone());
                    record.updated_at = now_timestamp();
                    self.storage.save_task(&record)?;
                }
            }
            Ok(())
        };
        if let Err(error) = save() {
            eprintln!("VIDEO_TERMINAL_SAVE(detail): {error}");
        }
    }
}

impl Drop for Execution {
    /// 异常丢弃或 panic 时停止剩余工作，并兜底终态；正常路径不重复清理。
    fn drop(&mut self) {
        if self.finished {
            return;
        }
        let error = if self.control.is_cancelled() {
            CommandError::new("VIDEO_CANCELLED", "视频任务已停止")
        } else {
            interrupted()
        };
        self.control.cancel();
        let task_id = self.control.snapshot().task_id;
        if let Err(error) = self.storage.cleanup_task_media(&task_id) {
            eprintln!("VIDEO_MEDIA_CLEANUP(detail): {error}");
        }
        self.finish(&Err(error));
    }
}

/// 只拥有调用者的取消责任，不拥有运行槽；执行者退出之前不能提前让后续任务进入。
struct CancelOnDrop(Option<VideoControl>);

impl Drop for CancelOnDrop {
    /// Agent 的 select 丢弃等待者时仍通知视频流水线走协作取消与完整收尾。
    fn drop(&mut self) {
        if let Some(control) = &self.0 {
            control.cancel();
        }
    }
}

/// 独立执行者不随 Agent 等待 future 一起被丢弃；正常调用仍等待完整结果。
pub(super) async fn wait<F>(
    control: VideoControl,
    future: F,
) -> Result<VideoTaskStatus, CommandError>
where
    F: Future<Output = Result<VideoTaskStatus, CommandError>> + Send + 'static,
{
    let mut cancellation = CancelOnDrop(Some(control));
    let result = tauri::async_runtime::spawn(future).await;
    cancellation.0 = None;
    result.map_err(|_| interrupted())?
}

/// 异常诊断不把 panic 内容或内部路径返回给工具调用方。
fn interrupted() -> CommandError {
    CommandError::new("VIDEO_INTERRUPTED", "视频任务执行中断，请重试")
}

#[cfg(test)]
#[path = "video_execution_tests.rs"]
mod tests;
