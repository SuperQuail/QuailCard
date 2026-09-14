//! 显式启用的真实 CLI 冒烟测试，不下载模型、不修改用户音频旁的文件。

use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use super::{output_path, output_prefix, AsrEngine, AsrOptions, WhisperCli};

struct Scratch(PathBuf);

impl Drop for Scratch {
    /// 失败断言同样清理临时转写目录，绝不触碰输入目录。
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// 由开发者提供本地二进制、模型及含人声音频；默认测试集不会运行。
#[tokio::test]
#[ignore = "requires WHISPER_SMOKE_PROGRAM, WHISPER_SMOKE_MODEL and WHISPER_SMOKE_WAV"]
async fn real_cli_transcribes_with_observed_backend() {
    let program =
        PathBuf::from(std::env::var_os("WHISPER_SMOKE_PROGRAM").expect("WHISPER_SMOKE_PROGRAM"));
    let model =
        PathBuf::from(std::env::var_os("WHISPER_SMOKE_MODEL").expect("WHISPER_SMOKE_MODEL"));
    let source = PathBuf::from(std::env::var_os("WHISPER_SMOKE_WAV").expect("WHISPER_SMOKE_WAV"));
    let scratch = Scratch(
        std::env::temp_dir().join(format!("quailcard-whisper-smoke-{}", uuid::Uuid::now_v7())),
    );
    std::fs::create_dir_all(&scratch.0).unwrap();
    let wav = scratch.0.join("smoke.clip.wav");
    std::fs::copy(source, &wav).unwrap();
    let options = AsrOptions {
        model,
        language: None,
        initial_prompt: None,
        disable_gpu: std::env::var("WHISPER_SMOKE_DISABLE_GPU")
            .is_ok_and(|value| value == "1" || value == "true"),
    };
    let names = Arc::new(Mutex::new(Vec::<String>::new()));
    let observed = names.clone();
    let transcript = WhisperCli::new(program)
        .transcribe_observed(
            &wav,
            &options,
            Arc::new(|| false),
            Arc::new(|_| {}),
            Arc::new(move |name| observed.lock().unwrap().push(name.to_string())),
        )
        .await
        .expect("real CLI transcription");
    assert!(
        !transcript.segments.is_empty(),
        "fixture must contain audible speech"
    );
    let names = names.lock().unwrap();
    assert!(
        !names.is_empty(),
        "CLI must provide recognized backend diagnostics"
    );
    assert!(names.iter().all(|name| matches!(
        name.as_str(),
        "cpu" | "vulkan" | "cuda" | "metal" | "sycl" | "hip" | "rocm" | "opencl"
    )));
    if let Ok(expected) = std::env::var("WHISPER_SMOKE_EXPECT_BACKEND") {
        assert_eq!(
            names.last().map(String::as_str),
            Some(expected.as_str()),
            "实际计算后端必须符合验收目标"
        );
    }
    eprintln!(
        "WHISPER_SMOKE segments={} backend={:?}",
        transcript.segments.len(),
        names.last()
    );
    let prefix = output_prefix(&wav);
    assert_eq!(prefix.parent(), wav.parent());
    assert!(output_path(&prefix)
        .to_string_lossy()
        .ends_with(".whisper.json"));
    assert!(
        std::fs::read_dir(&scratch.0)
            .unwrap()
            .all(|entry| entry.unwrap().path() == wav),
        "unique output must be cleaned after parsing"
    );
}
