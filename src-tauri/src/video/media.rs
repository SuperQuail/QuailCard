//! 媒体处理端口与子进程运行助手。

pub(crate) mod ffmpeg;
#[cfg(test)]
#[path = "media/frame_smoke_tests.rs"]
mod frame_smoke_tests;
mod proxy;

use std::path::{Path, PathBuf};
use std::sync::Arc;

use crate::error::CommandError;
use crate::video::transcript::Transcript;

/// 可克隆的取消判定；避免把借用跨异步边界传递。
pub(crate) type CancelHandle = Arc<dyn Fn() -> bool + Send + Sync>;

/// 进度回调句柄。
pub(crate) type ProgressHandle = Arc<dyn Fn(u8) + Send + Sync>;

/// 后端诊断仅传递白名单名称，禁止转发组件原始输出。
pub(crate) type BackendHandle = Arc<dyn Fn(&str) + Send + Sync>;

/// 领域层使用的异步结果别名。
pub(crate) type VideoFuture<'a, T> =
    std::pin::Pin<Box<dyn std::future::Future<Output = Result<T, CommandError>> + Send + 'a>>;

/// 抽帧来源；远端 headers 仅兼容旧接口，适配器不得用于传递账号凭据。
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum FrameSource {
    Local(PathBuf),
    Remote {
        url: String,
        headers: Vec<String>,
    },
    /// 顺序尝试主地址与备用地址；每次抽帧固定一个无凭据来源。
    RemoteCandidates {
        urls: Vec<String>,
    },
}

/// 音频导出端口：把任意媒体转成 16 kHz 单声道 WAV。
pub(crate) trait AudioExtractor: Send + Sync {
    /// 导出 WAV；取消时中止子进程并清理半成品。
    fn export_wav<'a>(
        &'a self,
        input: &'a Path,
        output: &'a Path,
        cancel: CancelHandle,
    ) -> VideoFuture<'a, ()>;
}

/// 抽帧端口：按时间点取一帧为 JPEG。
pub(crate) trait FrameExtractor: Send + Sync {
    /// 取帧并限制最长边；远端签名媒体经短命代理读取，不向子进程传递 Cookie。
    fn extract<'a>(
        &'a self,
        source: FrameSource,
        seconds: f64,
        output: &'a Path,
        max_width: u32,
        cancel: CancelHandle,
    ) -> VideoFuture<'a, ()>;
}

/// 本地语音识别端口。
pub(crate) trait AsrEngine: Send + Sync {
    /// 转写 WAV；进度回调为 0-100。
    fn transcribe<'a>(
        &'a self,
        wav: &'a Path,
        options: &'a AsrOptions,
        cancel: CancelHandle,
        progress: ProgressHandle,
    ) -> VideoFuture<'a, Transcript>;

    /// 可选诊断观察者；旧适配器保持兼容，不伪造实际后端。
    fn transcribe_observed<'a>(
        &'a self,
        wav: &'a Path,
        options: &'a AsrOptions,
        cancel: CancelHandle,
        progress: ProgressHandle,
        _backend: BackendHandle,
    ) -> VideoFuture<'a, Transcript> {
        self.transcribe(wav, options, cancel, progress)
    }
}

/// 一次转写的参数。
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct AsrOptions {
    pub model: PathBuf,
    pub language: Option<String>,
    pub initial_prompt: Option<String>,
    /// 明确禁用 GPU，用于 Vulkan 失败后的回退。
    pub disable_gpu: bool,
}

/// 运行子进程并在取消时终止；kill_on_drop 保证取消后不留孤儿进程。
pub(crate) async fn run_process(
    program: &Path,
    args: &[String],
    cancel: &CancelHandle,
) -> Result<std::process::Output, CommandError> {
    if cancel() {
        return Err(CommandError::new("VIDEO_CANCELLED", "已停止视频任务"));
    }
    let mut child = tokio::process::Command::new(program)
        .args(args)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .kill_on_drop(true)
        .spawn()
        .map_err(|error| {
            eprintln!("VIDEO_PROCESS_SPAWN(detail): {error}");
            CommandError::new(
                "VIDEO_COMPONENT_MISSING",
                "无法启动媒体组件，请重新安装或指定路径",
            )
        })?;
    tokio::select! {
        biased;
        _ = wait_for_cancel(cancel) => {
            // 优先回收再清理输出，超时仍由 kill_on_drop 兜底，避免取消无限等待。
            let _ = tokio::time::timeout(std::time::Duration::from_millis(200), child.kill()).await;
            Err(CommandError::new("VIDEO_CANCELLED", "已停止视频任务"))
        },
        result = child.wait() => result.map(|status| std::process::Output { status, stdout: Vec::new(), stderr: Vec::new() })
            .map_err(|_| CommandError::new("VIDEO_COMPONENT_FAILED", "媒体组件执行失败")),
    }
}

/// 轮询取消标记；取消后由 kill_on_drop 回收子进程。
pub(crate) async fn wait_for_cancel(cancel: &CancelHandle) {
    loop {
        if cancel() {
            return;
        }
        tokio::time::sleep(std::time::Duration::from_millis(200)).await;
    }
}
