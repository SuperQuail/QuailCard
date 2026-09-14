//! 可取消的 whisper-cli 子进程；诊断按有界行消费，不保存原始输出。

use std::path::Path;
use tokio::io::AsyncReadExt;

use super::diagnostics::Diagnostics;
use crate::error::CommandError;
use crate::video::media::{wait_for_cancel, BackendHandle, CancelHandle, ProgressHandle};

/// 返回安全错误；取消优先于退出和输出事件，并由 kill_on_drop 终止子进程。
pub(super) async fn run(
    program: &Path,
    args: &[String],
    cancel: &CancelHandle,
    progress: &ProgressHandle,
    backend: &BackendHandle,
    disable_gpu: bool,
) -> Result<(), CommandError> {
    if cancel() {
        return Err(cancelled());
    }
    let mut child = tokio::process::Command::new(program)
        .args(args)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::piped())
        .kill_on_drop(true)
        .spawn()
        .map_err(|_| {
            CommandError::new(
                "VIDEO_COMPONENT_MISSING",
                "无法启动语音组件，请重新安装或指定路径",
            )
        })?;
    let mut stderr = child.stderr.take().ok_or_else(failed)?;
    let mut diagnostics = Diagnostics::default();
    let mut chunk = [0u8; 4096];
    let mut line = Vec::new();
    let mut overflow = false;
    let mut eof = false;
    let mut status = None;
    while status.is_none() || !eof {
        tokio::select! {
            biased;
            _ = wait_for_cancel(cancel) => {
                // 先终止并短暂等待回收，避免 Windows 文件句柄阻止外层输出守卫清理。
                let _ = tokio::time::timeout(std::time::Duration::from_millis(200), child.kill()).await;
                return Err(cancelled());
            },
            result = child.wait(), if status.is_none() => {
                status = Some(result.map_err(|_| failed())?);
            }
            result = stderr.read(&mut chunk), if !eof => {
                let count = result.map_err(|_| failed())?;
                eof = count == 0;
                for &byte in &chunk[..count] {
                    if byte == b'\n' || byte == b'\r' {
                        if !overflow {
                            observe(&line, &mut diagnostics, progress, backend);
                        }
                        line.clear();
                        overflow = false;
                    } else if line.len() < 8192 {
                        line.push(byte);
                    } else {
                        // 超长行整体丢弃，避免提示词回显导致无限分配或伪诊断。
                        overflow = true;
                    }
                }
                if eof && !overflow {
                    observe(&line, &mut diagnostics, progress, backend);
                }
            }
        }
    }
    if cancel() {
        return Err(cancelled());
    }
    let loader_failed = cfg!(windows)
        && status
            .and_then(|value| value.code())
            .is_some_and(|code| matches!(code as u32, 0xc000_0135 | 0xc000_0139 | 0xc000_007b))
        && crate::video::components::cpu_fallback(program).is_some();
    if status.is_some_and(|status| status.success()) {
        Ok(())
    } else if (diagnostics.gpu_failed || loader_failed) && !disable_gpu {
        Err(CommandError::new(
            "VIDEO_ASR_GPU_FAILED",
            "GPU 语音后端运行失败",
        ))
    } else {
        Err(failed())
    }
}

/// 在本地解析后仅上报数值和白名单名称，原始行立即释放。
fn observe(
    line: &[u8],
    diagnostics: &mut Diagnostics,
    progress: &ProgressHandle,
    backend: &BackendHandle,
) {
    let line = String::from_utf8_lossy(line);
    if let Some(value) = super::parse_progress(&line) {
        progress(value.min(100));
    }
    let previous = diagnostics.backend;
    diagnostics.observe(&line);
    if diagnostics.backend != previous {
        if let Some(name) = diagnostics.backend {
            backend(name);
        }
    }
}

/// 所有取消路径使用同一安全错误，不混淆成转写失败。
pub(super) fn cancelled() -> CommandError {
    CommandError::new("VIDEO_CANCELLED", "已停止视频任务")
}

/// 通用失败不推测 GPU 原因，避免不必要的自动重试。
fn failed() -> CommandError {
    CommandError::new("VIDEO_ASR_FAILED", "本地转写失败，请检查音频与模型文件")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, Mutex};

    /// stderr 行观察只转发可信进度数值，不转发提示词中的同名文本。
    #[test]
    fn observes_only_whisper_progress_callback() {
        let values = Arc::new(Mutex::new(Vec::new()));
        let captured = values.clone();
        let progress: ProgressHandle = Arc::new(move |value| captured.lock().unwrap().push(value));
        let backend: BackendHandle = Arc::new(|_| {});
        let mut diagnostics = Diagnostics::default();
        observe(
            b"prompt: progress = 99%",
            &mut diagnostics,
            &progress,
            &backend,
        );
        observe(
            b"whisper_print_progress_callback: progress = 42%",
            &mut diagnostics,
            &progress,
            &backend,
        );
        observe(
            b"whisper_print_progress_callback: progress = 101%",
            &mut diagnostics,
            &progress,
            &backend,
        );
        assert_eq!(*values.lock().unwrap(), vec![42]);
    }
}
