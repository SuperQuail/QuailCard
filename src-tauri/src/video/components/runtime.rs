//! 以实际子进程输出探测组件，路径与构建清单不能证明 GPU 可用。

use std::path::{Path, PathBuf};
use std::process::Stdio;
use tokio::io::AsyncReadExt;

/// 探测结果仅含白名单信息，不把外部程序日志或用户路径暴露给界面。
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct RuntimeProbe {
    pub runnable: bool,
    pub gpu_backend: Option<String>,
    pub gpu_available: bool,
    pub detail: String,
}

/// 独立 CPU 包不链接 Vulkan loader，缺驱动时也能启动；自定义目录同样遵循此约定。
pub(crate) fn cpu_fallback(path: &Path) -> Option<PathBuf> {
    let candidate = path
        .parent()?
        .join("cpu")
        .join(super::Component::Whisper.file_name());
    candidate.is_file().then_some(candidate)
}

/// 先验证 help，再把可执行文件作为无效模型触发后端枚举；不读取用户模型或创建文件。
pub(crate) async fn probe_whisper(path: &Path) -> RuntimeProbe {
    let Some((success, mut output)) = bounded_probe(path, false).await else {
        return classify(false, "");
    };
    if classify(success, &output).runnable {
        if let Some((_, devices)) = bounded_probe(path, true).await {
            // 固定上游先枚举设备，再拒绝非模型 magic；该非零退出是预期探测结果。
            output.push('\n');
            output.push_str(&devices);
        }
    }
    classify(success, &output)
}

/// 同时消费两路管道，超时后 kill_on_drop 终止子进程；最多保留各 32 KiB。
async fn bounded_probe(path: &Path, initialize: bool) -> Option<(bool, String)> {
    let mut command = tokio::process::Command::new(path);
    if initialize {
        command.arg("-m").arg(path).arg("-f").arg(path);
    } else {
        command.arg("--help");
    }
    command
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    #[cfg(windows)]
    command.creation_flags(0x08000000);
    let mut child = command.spawn().ok()?;
    let mut stdout = child.stdout.take()?.take(32768);
    let mut stderr = child.stderr.take()?.take(32768);
    let result = tokio::time::timeout(std::time::Duration::from_secs(8), async {
        let mut out = Vec::new();
        let mut err = Vec::new();
        let (status, a, b) = tokio::join!(
            child.wait(),
            stdout.read_to_end(&mut out),
            stderr.read_to_end(&mut err)
        );
        a.ok()?;
        b.ok()?;
        out.extend_from_slice(&err);
        Some((
            status.ok()?.success(),
            String::from_utf8_lossy(&out).into_owned(),
        ))
    })
    .await
    .ok()
    .flatten();
    if result.is_none() {
        let _ = child.kill().await;
        let _ = child.wait().await;
    }
    result
}

/// 只接受真实设备条目，不把 --no-gpu 帮助文本或编译标记误判为可用 GPU。
fn classify(success: bool, output: &str) -> RuntimeProbe {
    let text = output.to_ascii_lowercase();
    let runnable = success && text.contains("usage:") && text.contains("--model");
    let vulkan_device = text.lines().any(|line| {
        let Some(rest) = line.trim().strip_prefix("ggml_vulkan: ") else {
            return false;
        };
        let Some((index, info)) = rest.split_once('=') else {
            return false;
        };
        index.trim().parse::<usize>().is_ok() && info.contains("uma:")
    });
    let gpu_available = runnable && vulkan_device && !text.contains("device type is cpu");
    let gpu_backend = text.contains("ggml_vulkan:").then(|| "Vulkan".to_owned());
    let detail = if !runnable {
        "组件启动失败或自检超时；请检查架构、运行库和驱动，转写可尝试独立 CPU 组件"
    } else if gpu_available {
        "Vulkan 已枚举到设备；实际模型计算后端以转写诊断为准，失败自动回退 CPU"
    } else if gpu_backend.is_some() {
        "Vulkan 未检测到可用设备；使用 CPU，建议更新显卡驱动"
    } else {
        "组件可运行；自检未确认 GPU 设备，实际后端以转写诊断为准"
    };
    RuntimeProbe {
        runnable,
        gpu_backend,
        gpu_available,
        detail: detail.into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    const HELP: &str = "usage: whisper-cli [options] --model FNAME --no-gpu";

    /// 帮助中提到 GPU 不意味着二进制有可用设备。
    #[test]
    fn cpu_help_is_not_gpu() {
        let probe = classify(true, HELP);
        assert!(probe.runnable);
        assert!(!probe.gpu_available);
        assert_eq!(probe.gpu_backend, None);
    }

    /// 仅接受上游实际枚举条目，零设备和崩溃均不冒充 GPU 可用。
    #[test]
    fn vulkan_requires_device_and_success() {
        let log = format!("{HELP}\nggml_vulkan: 0 = NVIDIA (driver) | uma: 0 | fp16: 1");
        assert!(classify(true, &log).gpu_available);
        assert!(!classify(false, &log).gpu_available);
        assert!(!classify(true, &format!("{HELP}\nggml_vulkan: No devices found.")).gpu_available);
    }

    /// 异常输出与路径不能直接变成 UI 消息。
    #[test]
    fn probe_does_not_expose_logs() {
        let probe = classify(false, "secret-path api-key");
        assert!(!probe.detail.contains("secret"));
        assert!(!probe.runnable);
    }

    /// 显式本机集成测试，避免在没有 GPU 的 CI 上伪造运行时验证。
    #[tokio::test]
    #[ignore = "requires QC_WHISPER_PROBE pointing to a Vulkan binary and an actual GPU"]
    async fn installed_vulkan_device_probe() {
        let path = std::env::var_os("QC_WHISPER_PROBE").expect("QC_WHISPER_PROBE");
        let probe = probe_whisper(Path::new(&path)).await;
        assert!(probe.runnable, "{}", probe.detail);
        assert_eq!(probe.gpu_backend.as_deref(), Some("Vulkan"));
        assert!(probe.gpu_available, "{}", probe.detail);
    }

    /// 独立 CPU 仅从同级约定目录解析，避免再次回退到 GPU 或递归使用自己。
    #[test]
    fn independent_cpu_path_is_resolved() {
        let directory = std::env::temp_dir().join(format!("qc-cpu-{}", uuid::Uuid::now_v7()));
        std::fs::create_dir_all(directory.join("cpu")).unwrap();
        let gpu = directory.join(super::super::Component::Whisper.file_name());
        let cpu = directory
            .join("cpu")
            .join(super::super::Component::Whisper.file_name());
        std::fs::write(&cpu, b"fixture").unwrap();
        assert_eq!(cpu_fallback(&gpu), Some(cpu.clone()));
        assert_eq!(cpu_fallback(&cpu), None);
        std::fs::remove_dir_all(directory).unwrap();
    }

    /// 不存在的独立组件必须显式缺失，不复用依赖损坏的 GPU 文件。
    #[test]
    fn missing_cpu_fallback_is_none() {
        assert!(cpu_fallback(Path::new("/not-present/whisper-cli")).is_none());
    }
}
