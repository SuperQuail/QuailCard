//! whisper-cli 适配器：调用子进程转写并解析 JSON 结果。

#[path = "whisper_diagnostics.rs"]
mod diagnostics;
#[path = "whisper_process.rs"]
mod process;
#[cfg(test)]
#[path = "whisper_smoke.rs"]
mod smoke;

use std::path::{Path, PathBuf};

use super::super::media::{
    wait_for_cancel, AsrEngine, AsrOptions, BackendHandle, CancelHandle, ProgressHandle,
    VideoFuture,
};
use crate::error::CommandError;
use crate::video::transcript::{Segment, Transcript};

/// 绑定 whisper-cli 可执行文件的转写引擎。
pub(crate) struct WhisperCli {
    program: PathBuf,
}

impl WhisperCli {
    /// 使用解析到的可执行文件路径创建引擎。
    pub(crate) fn new(program: PathBuf) -> Self {
        Self { program }
    }

    /// 组装命令行参数；具体开关以随包版本的用法为准。
    fn args(wav: &Path, options: &AsrOptions, prefix: &Path) -> Vec<String> {
        let mut args = vec![
            "-m".to_string(),
            options.model.to_string_lossy().to_string(),
            "-f".to_string(),
            wav.to_string_lossy().to_string(),
            "-of".to_string(),
            prefix.to_string_lossy().to_string(),
            "-oj".to_string(),
            "-bs".to_string(),
            "5".to_string(),
            "-pp".to_string(),
        ];
        args.push("-l".to_string());
        args.push(
            options
                .language
                .clone()
                .unwrap_or_else(|| "auto".to_string()),
        );
        if let Some(prompt) = options
            .initial_prompt
            .as_ref()
            .filter(|value| !value.trim().is_empty())
        {
            args.push("--prompt".to_string());
            args.push(prompt.chars().take(200).collect());
        }
        if options.disable_gpu {
            args.push("-ng".to_string());
        }
        args
    }
}

impl AsrEngine for WhisperCli {
    /// 兼容不需要诊断的调用者，所有执行逻辑共用可观察入口。
    fn transcribe<'a>(
        &'a self,
        wav: &'a Path,
        options: &'a AsrOptions,
        cancel: CancelHandle,
        progress: ProgressHandle,
    ) -> VideoFuture<'a, Transcript> {
        self.transcribe_observed(wav, options, cancel, progress, std::sync::Arc::new(|_| {}))
    }

    /// 仅从组件诊断观察实际后端；参数启用 GPU 不代表实际使用 GPU。
    fn transcribe_observed<'a>(
        &'a self,
        wav: &'a Path,
        options: &'a AsrOptions,
        cancel: CancelHandle,
        progress: ProgressHandle,
        backend: BackendHandle,
    ) -> VideoFuture<'a, Transcript> {
        Box::pin(async move {
            if cancel() {
                return Err(process::cancelled());
            }
            if !options.model.is_file() {
                return Err(CommandError::new(
                    "VIDEO_ASR_MODEL_MISSING",
                    "语音模型缺失，请先在设置中下载",
                ));
            }
            let prefix = output_prefix(wav);
            let json_path = output_path(&prefix);
            let _output_guard = OutputGuard(json_path.clone());
            let args = Self::args(wav, options, &prefix);
            // Vulkan 依赖加载失败时必须换 CPU 二进制，单加 -ng 无法绕过动态加载器。
            let cpu_program = options
                .disable_gpu
                .then(|| crate::video::components::cpu_fallback(&self.program))
                .flatten();
            let program = cpu_program.as_deref().unwrap_or(&self.program);
            process::run(
                program,
                &args,
                &cancel,
                &progress,
                &backend,
                options.disable_gpu,
            )
            .await?;
            let output = async {
                let payload = tokio::fs::read_to_string(&json_path)
                    .await
                    .map_err(|_| CommandError::new("VIDEO_ASR_FAILED", "转写结果文件缺失"))?;
                // 大结果解析移出运行时线程，取消不必等待 CPU 密集的 JSON 解析。
                tokio::task::spawn_blocking(move || parse_transcription(&payload))
                    .await
                    .map_err(|_| CommandError::new("VIDEO_ASR_FAILED", "转写结果无法解析"))?
            };
            tokio::select! {
                biased;
                _ = wait_for_cancel(&cancel) => Err(process::cancelled()),
                result = output => {
                    if cancel() { Err(process::cancelled()) } else { result }
                }
            }
        })
    }
}

struct OutputGuard(PathBuf);

impl Drop for OutputGuard {
    /// 所有退出路径只删除本次唯一输出，不删除旧任务或用户的同名 JSON。
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}

/// 同一音频重复转写也使用唯一前缀，避免失败时误读前次遗留结果。
fn output_prefix(wav: &Path) -> PathBuf {
    wav.with_extension(format!("{}.whisper", uuid::Uuid::now_v7()))
}

/// whisper-cli 的 -of 是完整前缀，必须追加扩展名而不是替换原后缀。
fn output_path(prefix: &Path) -> PathBuf {
    let mut path = prefix.as_os_str().to_os_string();
    path.push(".json");
    PathBuf::from(path)
}

/// 解析进度输出：whisper_print_progress_callback: progress = 42%
pub(crate) fn parse_progress(line: &str) -> Option<u8> {
    let rest = line
        .trim()
        .strip_prefix("whisper_print_progress_callback:")?
        .trim();
    let digits = rest
        .strip_prefix("progress =")?
        .trim()
        .strip_suffix('%')?
        .trim();
    if digits.is_empty() || !digits.chars().all(|character| character.is_ascii_digit()) {
        return None;
    }
    digits.parse::<u8>().ok().filter(|value| *value <= 100)
}

/// 解析 whisper-cli 的 JSON 输出；容忍 offsets 或 timestamps 两种字段。
pub(crate) fn parse_transcription(payload: &str) -> Result<Transcript, CommandError> {
    let value: serde_json::Value = serde_json::from_str(payload)
        .map_err(|_| CommandError::new("VIDEO_ASR_FAILED", "转写结果无法解析"))?;
    let items = value
        .get("transcription")
        .and_then(|items| items.as_array())
        .cloned()
        .unwrap_or_default();
    let language = value
        .get("result")
        .and_then(|result| result.get("language"))
        .and_then(|language| language.as_str())
        .unwrap_or("")
        .to_string();
    let mut segments = Vec::new();
    for item in items {
        let text = item
            .get("text")
            .and_then(|text| text.as_str())
            .unwrap_or("")
            .trim()
            .to_string();
        if text.is_empty() {
            continue;
        }
        let start = item
            .get("offsets")
            .and_then(|offsets| offsets.get("from"))
            .and_then(|from| from.as_f64())
            .map(|milliseconds| milliseconds / 1000.0)
            .or_else(|| {
                item.get("timestamps")
                    .and_then(|stamps| stamps.get("from"))
                    .and_then(|from| from.as_str())
                    .and_then(parse_clock)
            })
            .unwrap_or(0.0);
        let end = item
            .get("offsets")
            .and_then(|offsets| offsets.get("to"))
            .and_then(|to| to.as_f64())
            .map(|milliseconds| milliseconds / 1000.0)
            .or_else(|| {
                item.get("timestamps")
                    .and_then(|stamps| stamps.get("to"))
                    .and_then(|to| to.as_str())
                    .and_then(parse_clock)
            })
            .unwrap_or(start);
        segments.push(Segment {
            start,
            end: end.max(start),
            text,
        });
    }
    if segments.is_empty() {
        return Err(CommandError::new(
            "VIDEO_TRANSCRIPT_EMPTY",
            "没有识别到有效语音",
        ));
    }
    Ok(Transcript {
        language,
        source: "whisper".to_string(),
        segments,
    })
}

/// 解析 hh:mm:ss,fff 形式的时间戳。
fn parse_clock(value: &str) -> Option<f64> {
    let parts: Vec<&str> = value.split([':', ',']).collect();
    let seconds = match parts.len() {
        4 => {
            parts[0].parse::<f64>().ok()? * 3600.0
                + parts[1].parse::<f64>().ok()? * 60.0
                + parts[2].parse::<f64>().ok()?
                + parts[3].parse::<f64>().ok()? / 1000.0
        }
        3 => {
            parts[0].parse::<f64>().ok()? * 60.0
                + parts[1].parse::<f64>().ok()?
                + parts[2].parse::<f64>().ok()? / 1000.0
        }
        _ => return None,
    };
    Some(seconds)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 唯一输出与音频同目录并保留 whisper 后缀，重复任务不会共享结果。
    #[test]
    fn unique_output_prefix_and_guard_cleanup() {
        let wav = std::env::temp_dir().join("quailcard-prefix-test.wav");
        let first = output_prefix(&wav);
        let second = output_prefix(&wav);
        assert_ne!(first, second);
        assert_eq!(first.parent(), wav.parent());
        assert_eq!(first.extension().unwrap(), "whisper");
        let path = output_path(&first);
        {
            let _guard = OutputGuard(path.clone());
            std::fs::write(&path, "partial output").unwrap();
        }
        assert!(!path.exists());
    }

    /// 点号前缀必须完整保留，防止误读同目录其他 JSON 文件。
    #[test]
    fn appends_json_to_complete_prefix() {
        assert_eq!(
            output_path(Path::new("clip.whisper")),
            PathBuf::from("clip.whisper.json")
        );
        assert_eq!(
            output_path(Path::new("clip.part.whisper")),
            PathBuf::from("clip.part.whisper.json")
        );
    }

    #[test]
    /// 解析 offsets（毫秒）与语言信息。
    fn parses_offsets_output() {
        let payload = r#"{"result":{"language":"zh"},"transcription":[{"offsets":{"from":0,"to":2500},"text":" 你好 "},{"offsets":{"from":2500,"to":4000},"text":""}]}"#;
        let transcript = parse_transcription(payload).unwrap();
        assert_eq!(transcript.language, "zh");
        assert_eq!(transcript.segments.len(), 1);
        assert_eq!(transcript.segments[0].text, "你好");
        assert_eq!(transcript.segments[0].end, 2.5);
    }

    #[test]
    /// 缺少 offsets 时回退到时间戳字符串。
    fn falls_back_to_timestamps() {
        let payload = r#"{"transcription":[{"timestamps":{"from":"00:01:05,500","to":"00:01:07,000"},"text":"回退"}]}"#;
        let transcript = parse_transcription(payload).unwrap();
        assert_eq!(transcript.segments[0].start, 65.5);
    }

    #[test]
    /// 进度解析容错。
    fn parses_progress_lines() {
        assert_eq!(
            parse_progress("whisper_print_progress_callback: progress = 42%"),
            Some(42)
        );
        assert_eq!(parse_progress("whisper_full_with_state: some info"), None);
        for line in [
            "prompt: progress = 42%",
            "whisper_print_progress_callback: progress = 101%",
            "whisper_print_progress_callback: progress = 42",
            "whisper_print_progress_callback: progress = 42% extra",
        ] {
            assert_eq!(parse_progress(line), None);
        }
        assert_eq!(
            parse_progress("whisper_print_progress_callback: progress = 100%"),
            Some(100)
        );
    }
}
