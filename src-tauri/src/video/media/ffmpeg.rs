//! ffmpeg 适配器：远端签名媒体通过短命回环 Range 代理按需读取。
use super::{run_process, AudioExtractor, CancelHandle, FrameExtractor, FrameSource, VideoFuture};
use crate::error::CommandError;
use std::path::{Path, PathBuf};

/// 仅清理本次随机生成的文件，不删除已有目标或任务目录。
struct Temporary(PathBuf);
impl Temporary {
    /// 临时文件保留真实扩展名，让 ffmpeg 正确选择输出容器。
    fn beside(output: &Path, extension: &str) -> Self {
        Self(output.with_file_name(format!(".qc-{}.{}", uuid::Uuid::now_v7(), extension)))
    }
}
impl Drop for Temporary {
    /// 错误、取消和未来被丢弃时均清理本次临时文件。
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}

/// 绑定 ffmpeg 可执行文件的媒体工具。
pub(crate) struct Ffmpeg {
    program: PathBuf,
}
impl Ffmpeg {
    /// 使用解析到的 ffmpeg 路径创建工具集。
    pub(crate) fn new(program: PathBuf) -> Self {
        Self { program }
    }

    /// 子进程禁用网络协议，防止本地伪媒体引用远端或内网地址。
    fn input_args(input: &Path) -> Vec<String> {
        vec![
            "-protocol_whitelist".into(),
            "file".into(),
            "-i".into(),
            input.to_string_lossy().into_owned(),
        ]
    }

    /// 先生成完整临时输出再替换，stderr 可能含签名和转录隐私，禁止记录。
    async fn finish(
        &self,
        args: &[String],
        temporary: &Path,
        output: &Path,
        cancel: &CancelHandle,
        code: &'static str,
        message: &str,
    ) -> Result<(), CommandError> {
        let result = run_process(&self.program, args, cancel).await?;
        if !result.status.success() || !temporary.is_file() {
            return Err(CommandError::new(code, message));
        }
        if cancel() {
            return Err(CommandError::new("VIDEO_CANCELLED", "已停止视频任务"));
        }
        tokio::fs::rename(temporary, output).await?;
        Ok(())
    }
}

impl AudioExtractor for Ffmpeg {
    /// 输出单声道 PCM WAV；任何失败均保留旧文件并清理半成品。
    fn export_wav<'a>(
        &'a self,
        input: &'a Path,
        output: &'a Path,
        cancel: CancelHandle,
    ) -> VideoFuture<'a, ()> {
        Box::pin(async move {
            let temporary = Temporary::beside(output, "wav");
            let mut args = vec!["-hide_banner".into(), "-nostdin".into(), "-y".into()];
            args.extend(Self::input_args(input));
            args.extend(
                ["-vn", "-ac", "1", "-ar", "16000", "-c:a", "pcm_s16le"]
                    .into_iter()
                    .map(String::from),
            );
            args.push(temporary.0.to_string_lossy().into_owned());
            self.finish(
                &args,
                &temporary.0,
                output,
                &cancel,
                "VIDEO_DECODE_FAILED",
                "音频解码失败，请尝试其他清晰度或稍后重试",
            )
            .await
        })
    }
}

impl Ffmpeg {
    /// 一次完整抽帧只绑定一个来源；代理和临时输出独立，失败不污染下一次。
    async fn extract_one(
        &self,
        source: FrameSource,
        seconds: f64,
        output: &Path,
        max_width: u32,
        cancel: CancelHandle,
    ) -> Result<(), CommandError> {
        if !seconds.is_finite() {
            return Err(CommandError::new("VIDEO_FRAME_FAILED", "截图时间无效"));
        }
        if cancel() {
            return Err(CommandError::new("VIDEO_CANCELLED", "已停止视频任务"));
        }
        // 代理守卫保持到子进程结束，退出时撤销监听和所有上游连接。
        let proxy;
        let input_args = match source {
            FrameSource::Local(path) => Self::input_args(&path),
            FrameSource::Remote { url, headers: _ } => {
                // 建立代理时尚无子进程，可安全取消；运行后必须等待组件自行回收。
                proxy = tokio::select! {
                    biased;
                    _ = super::wait_for_cancel(&cancel) => {
                        return Err(CommandError::new("VIDEO_CANCELLED", "已停止视频任务"));
                    }
                    result = super::proxy::start(url) => result?,
                };
                // 强制 MOV 解复用，不允许伪播放列表触发任意网络请求。
                vec![
                    "-protocol_whitelist".into(),
                    "http,tcp".into(),
                    "-f".into(),
                    "mov".into(),
                    "-i".into(),
                    proxy.url(),
                ]
            }
            FrameSource::RemoteCandidates { .. } => {
                return Err(CommandError::new("VIDEO_FRAME_FAILED", "截图来源无效"));
            }
        };
        let frame = Temporary::beside(output, "jpg");
        let mut args = vec![
            "-hide_banner".into(),
            "-nostdin".into(),
            "-y".into(),
            "-ss".into(),
            format!("{:.3}", seconds.max(0.0)),
        ];
        args.extend(input_args);
        args.extend(["-frames:v".into(), "1".into()]);
        if max_width > 0 {
            args.extend(["-vf".into(), format!("scale='min({max_width},iw)':-2")]);
        }
        args.extend([
            "-q:v".into(),
            "2".into(),
            frame.0.to_string_lossy().into_owned(),
        ]);
        self.finish(
            &args,
            &frame.0,
            output,
            &cancel,
            "VIDEO_FRAME_FAILED",
            "该时间点截图失败",
        )
        .await
    }
}

const MAX_FRAME_CANDIDATES: usize = 4;
const FRAME_CANDIDATE_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(90);

/// 每次重新执行完整操作，绝不在同一媒体流中换源；取消和缺失组件不是地址故障。
async fn try_candidates<F, Fut>(
    urls: Vec<String>,
    cancel: &CancelHandle,
    mut operation: F,
) -> Result<(), CommandError>
where
    F: FnMut(String, CancelHandle) -> Fut,
    Fut: std::future::Future<Output = Result<(), CommandError>>,
{
    let mut last_error = CommandError::new("VIDEO_FRAME_FAILED", "没有可用的截图来源");
    for url in urls.into_iter().take(MAX_FRAME_CANDIDATES) {
        if cancel() {
            return Err(CommandError::new("VIDEO_CANCELLED", "已停止视频任务"));
        }
        let deadline = tokio::time::Instant::now() + FRAME_CANDIDATE_TIMEOUT;
        let parent_cancel = cancel.clone();
        let attempt_cancel: CancelHandle =
            std::sync::Arc::new(move || parent_cancel() || tokio::time::Instant::now() >= deadline);
        // 操作必须遵守取消句柄；等进程回收及文件提交完成，不丢弃正在运行或重命名的 future。
        let result = operation(url, attempt_cancel).await;
        if cancel() {
            return Err(CommandError::new("VIDEO_CANCELLED", "已停止视频任务"));
        }
        match result {
            Ok(()) => return Ok(()),
            Err(error)
                if error.code == "VIDEO_CANCELLED" && tokio::time::Instant::now() >= deadline =>
            {
                last_error = CommandError::new("VIDEO_FRAME_FAILED", "该截图来源读取超时");
            }
            Err(error) if matches!(error.code, "VIDEO_CANCELLED" | "VIDEO_COMPONENT_MISSING") => {
                return Err(error);
            }
            Err(error) => last_error = error,
        }
    }
    if cancel() {
        return Err(CommandError::new("VIDEO_CANCELLED", "已停止视频任务"));
    }
    Err(last_error)
}

impl FrameExtractor for Ffmpeg {
    /// 旧来源保持单次行为；新候选来源有界重试，签名及 Cookie 始终不进入进程参数。
    fn extract<'a>(
        &'a self,
        source: FrameSource,
        seconds: f64,
        output: &'a Path,
        max_width: u32,
        cancel: CancelHandle,
    ) -> VideoFuture<'a, ()> {
        Box::pin(async move {
            match source {
                FrameSource::RemoteCandidates { urls } => {
                    try_candidates(urls, &cancel, |url, attempt_cancel| {
                        self.extract_one(
                            FrameSource::Remote {
                                url,
                                headers: Vec::new(),
                            },
                            seconds,
                            output,
                            max_width,
                            attempt_cancel,
                        )
                    })
                    .await
                }
                source => {
                    self.extract_one(source, seconds, output, max_width, cancel)
                        .await
                }
            }
        })
    }
}

#[cfg(test)]
#[path = "retry_tests.rs"]
mod retry_tests;

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    /// 进程仅接收本地输入且白名单不允许隐式 HTTP 请求。
    fn subprocess_input_is_local_only() {
        assert_eq!(
            Ffmpeg::input_args(Path::new("audio.wav")),
            ["-protocol_whitelist", "file", "-i", "audio.wav"]
        );
    }
}
