//! 模型下载事务：独占、完整性校验和取消清理。
//! 精确字节数与魔数只能检测损坏，不等价于密码学验证；未核验官方 SHA 前不伪造摘要。
use super::{has_ggml_magic, model_file, status, ModelOption};
use crate::error::CommandError;
use futures_util::StreamExt;
use std::{
    collections::HashSet,
    path::{Path, PathBuf},
    sync::{Mutex, OnceLock},
    time::Duration,
};
use tokio::io::AsyncWriteExt;

static ACTIVE: OnceLock<Mutex<HashSet<PathBuf>>> = OnceLock::new();

struct DownloadGuard {
    target: PathBuf,
    partial: PathBuf,
}
impl Drop for DownloadGuard {
    /// 所有退出路径均移除本次随机临时文件，不触碰其他下载或旧模型。
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.partial);
        if let Ok(mut active) = ACTIVE.get_or_init(Default::default).lock() {
            active.remove(&self.target);
        }
    }
}

/// 镜像只接受无凭据的 HTTPS 域名；拒绝字面本地地址和协议混淆。
fn safe_url(url: &reqwest::Url) -> bool {
    url.scheme() == "https"
        && url.username().is_empty()
        && url.password().is_none()
        && url.port_or_known_default() == Some(443)
        && url.host_str().is_some_and(|host| {
            host.contains('.')
                && !host.ends_with('.')
                && host.parse::<std::net::IpAddr>().is_err()
                && !host.ends_with(".localhost")
                && !host.ends_with(".local")
        })
}

/// 固定安全错误不包含镜像 URL、签名或底层网络原文。
fn network_error() -> CommandError {
    CommandError::new(
        "VIDEO_ASR_MODEL_MISSING",
        "模型下载失败，请检查网络或更换镜像地址",
    )
}

/// 即使连接或响应体永久停顿，也每百毫秒观察取消标记。
async fn cancelled(cancel: &(dyn Fn() -> bool + Send + Sync)) {
    while !cancel() {
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
}

/// 精确大小与 GGML 魔数验证通过后才原子替换；调用取消时自动清理临时文件。
pub(crate) async fn download(
    option: &ModelOption,
    data_dir: &Path,
    mirror: Option<&str>,
    cancel: &(dyn Fn() -> bool + Send + Sync),
    progress: &mut (dyn FnMut(u64, Option<u64>) + Send),
) -> Result<PathBuf, CommandError> {
    if cancel() {
        return Err(CommandError::new("VIDEO_CANCELLED", "已取消模型下载"));
    }
    let target = model_file(data_dir, option);
    let parent = target.parent().ok_or_else(network_error)?;
    std::fs::create_dir_all(parent)?;
    let target = std::fs::canonicalize(parent)?.join(option.file_name);
    {
        let mut active = ACTIVE
            .get_or_init(Default::default)
            .lock()
            .map_err(|_| network_error())?;
        if !active.insert(target.clone()) {
            return Err(CommandError::new(
                "VIDEO_MODEL_DOWNLOADING",
                "此模型正在下载",
            ));
        }
    }
    let guard = DownloadGuard {
        partial: target.with_extension(format!("{}.part", uuid::Uuid::now_v7())),
        target: target.clone(),
    };
    if status(data_dir, option) == "ready" {
        return Ok(target);
    }
    let base = mirror
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .unwrap_or("https://huggingface.co");
    let base = reqwest::Url::parse(base).map_err(|_| network_error())?;
    if !safe_url(&base) || base.query().is_some() || base.fragment().is_some() {
        return Err(CommandError::new(
            "VIDEO_INVALID_URL",
            "模型镜像必须是无凭据的 HTTPS 地址",
        ));
    }
    let url = format!(
        "{}/ggerganov/whisper.cpp/resolve/main/{}",
        base.as_str().trim_end_matches('/'),
        option.file_name
    );
    let transfer = async {
        let http = reqwest::Client::builder()
            .connect_timeout(Duration::from_secs(20))
            .read_timeout(Duration::from_secs(60))
            .redirect(reqwest::redirect::Policy::custom(|attempt| {
                if attempt.previous().len() >= 10 || !safe_url(attempt.url()) {
                    attempt.error("模型重定向不安全")
                } else {
                    attempt.follow()
                }
            }))
            .build()
            .map_err(|_| network_error())?;
        let response = http.get(url).send().await.map_err(|_| network_error())?;
        if !response.status().is_success() {
            return Err(network_error());
        }
        if response.content_length().is_some_and(|n| n != option.bytes) {
            return Err(CommandError::new(
                "VIDEO_ASR_MODEL_INVALID",
                "模型大小与官方清单不符",
            ));
        }
        let mut file = tokio::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&guard.partial)
            .await?;
        let mut stream = response.bytes_stream();
        let mut written = 0u64;
        while let Some(chunk) = stream.next().await {
            let chunk = chunk.map_err(|_| network_error())?;
            written = written
                .checked_add(chunk.len() as u64)
                .ok_or_else(network_error)?;
            if written > option.bytes {
                return Err(CommandError::new(
                    "VIDEO_ASR_MODEL_INVALID",
                    "模型文件超过预期大小",
                ));
            }
            file.write_all(&chunk).await?;
            progress(written, Some(option.bytes));
        }
        file.flush().await?;
        file.sync_all().await?;
        drop(file);
        if written != option.bytes || !has_ggml_magic(&guard.partial) {
            return Err(CommandError::new(
                "VIDEO_ASR_MODEL_INVALID",
                "模型文件不完整或格式错误，请重新下载",
            ));
        }
        if cancel() {
            return Err(CommandError::new("VIDEO_CANCELLED", "已取消模型下载"));
        }
        tokio::fs::rename(&guard.partial, &target).await?;
        Ok(target.clone())
    };
    tokio::select! {
        biased;
        _ = cancelled(cancel) => Err(CommandError::new("VIDEO_CANCELLED", "已取消模型下载")),
        result = transfer => result,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    /// URL 凭据、协议、本地地址和混淆主机不可进入模型请求。
    fn rejects_unsafe_mirrors() {
        for url in [
            "http://huggingface.co",
            "https://a:b@huggingface.co",
            "https://127.0.0.1",
            "https://localhost",
            "https://a.local",
            "https://huggingface.co:444",
        ] {
            assert!(!safe_url(&reqwest::Url::parse(url).unwrap()), "{url}");
        }
        assert!(safe_url(
            &reqwest::Url::parse("https://huggingface.co").unwrap()
        ));
    }
    #[tokio::test]
    /// 无数据的传输也必须被独立取消观察者打断。
    async fn cancellation_interrupts_stalled_future() {
        let flag = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let setter = flag.clone();
        let task = tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(20)).await;
            setter.store(true, std::sync::atomic::Ordering::Relaxed);
        });
        let check = || flag.load(std::sync::atomic::Ordering::Relaxed);
        tokio::time::timeout(Duration::from_millis(500), async {
            tokio::select! {
                _ = cancelled(&check) => {},
                _ = std::future::pending::<()>() => panic!("stalled transfer completed"),
            }
        })
        .await
        .unwrap();
        task.await.unwrap();
    }
    #[test]
    /// 事务清理只影响本次部分文件并释放互斥键，不破坏旧模型。
    fn guard_cleans_partial_and_unlocks() {
        let dir = std::env::temp_dir().join(format!("qc-guard-{}", uuid::Uuid::now_v7()));
        std::fs::create_dir_all(&dir).unwrap();
        let target = dir.join("model.bin");
        let partial = dir.join("unique.part");
        std::fs::write(&target, b"old").unwrap();
        std::fs::write(&partial, b"partial").unwrap();
        ACTIVE
            .get_or_init(Default::default)
            .lock()
            .unwrap()
            .insert(target.clone());
        drop(DownloadGuard {
            target: target.clone(),
            partial: partial.clone(),
        });
        assert!(!partial.exists());
        assert!(target.exists());
        assert!(!ACTIVE.get().unwrap().lock().unwrap().contains(&target));
        std::fs::remove_dir_all(dir).unwrap();
    }
    #[tokio::test]
    /// 已取消请求不得创建目录或发起网络访问。
    async fn pre_cancel_does_not_write() {
        let dir = std::env::temp_dir().join(format!("qc-cancel-{}", uuid::Uuid::now_v7()));
        let error = download(
            &super::super::MODELS[0],
            &dir,
            None,
            &|| true,
            &mut |_, _| {},
        )
        .await
        .unwrap_err();
        assert_eq!(error.code, "VIDEO_CANCELLED");
        assert!(!dir.exists());
    }
}
