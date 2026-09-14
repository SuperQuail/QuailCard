//! 下载生命周期：随机临时文件、原子替换与覆盖全部等待阶段的取消。
use super::http::BiliClient;
use crate::error::CommandError;
use futures_util::StreamExt;
use std::path::{Path, PathBuf};

pub(crate) type CancelFn = dyn Fn() -> bool + Send + Sync;
pub(crate) type ProgressFn = dyn FnMut(u64, Option<u64>) + Send;

/// 临时文件守卫在错误、取消和 future 被丢弃时均清理；成功 rename 后路径自然消失。
struct PartialFile(PathBuf);
impl Drop for PartialFile {
    /// 清理只针对本次随机路径，不会删除其他任务的下载结果。
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}

/// 取消覆盖连接、响应头、读取和写盘，不依赖下一块网络数据到达。
pub(crate) async fn download(
    client: &BiliClient,
    url: &str,
    referer: &str,
    target: &Path,
    cancelled: &CancelFn,
    progress: &mut ProgressFn,
) -> Result<u64, CommandError> {
    tokio::select! {
        biased;
        _ = observe_cancel(cancelled) => Err(cancelled_error()),
        result = download_inner(client, url, referer, target, progress) => result,
    }
}

/// 每百毫秒观察取消，独立于可能永远阻塞的上游或磁盘 future。
async fn observe_cancel(cancelled: &CancelFn) {
    while !cancelled() {
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    }
}

/// 数据完整后才发布目标文件；守卫先于文件句柄创建以保证 Windows 清理顺序。
async fn download_inner(
    client: &BiliClient,
    url: &str,
    referer: &str,
    target: &Path,
    progress: &mut ProgressFn,
) -> Result<u64, CommandError> {
    let response = client.stream(url, referer).await?;
    let total = response.content_length();
    let partial =
        PartialFile(target.with_file_name(format!(".quail-video-{}.part", uuid::Uuid::now_v7())));
    // 创建同步完成后再交给异步读写，避免取消打开 future 后后台遗留临时文件。
    let file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&partial.0)
        .map_err(|_| failed("无法创建下载文件"))?;
    let mut file = tokio::fs::File::from_std(file);
    let mut written = 0u64;
    let mut stream = response.bytes_stream();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(|_| failed("下载过程中断，请重试"))?;
        written = written
            .checked_add(chunk.len() as u64)
            .ok_or_else(|| failed("下载数据过大"))?;
        if total.is_some_and(|total| written > total) {
            return Err(failed("下载数据长度不一致"));
        }
        tokio::io::AsyncWriteExt::write_all(&mut file, &chunk)
            .await
            .map_err(|_| failed("写入下载文件失败"))?;
        progress(written, total);
    }
    if total.is_some_and(|total| total != written) {
        return Err(failed("下载数据不完整，请重试"));
    }
    tokio::io::AsyncWriteExt::flush(&mut file)
        .await
        .map_err(|_| failed("写入下载文件失败"))?;
    file.sync_all()
        .await
        .map_err(|_| failed("保存下载文件失败"))?;
    drop(file);
    tokio::fs::rename(&partial.0, target)
        .await
        .map_err(|_| failed("保存下载文件失败"))?;
    Ok(written)
}

/// 只尝试有限备用地址，取消不能被后续失败覆盖或重新发起请求。
pub(crate) async fn download_first(
    client: &BiliClient,
    urls: &[String],
    referer: &str,
    target: &Path,
    cancelled: &CancelFn,
    progress: &mut ProgressFn,
) -> Result<u64, CommandError> {
    let mut last = None;
    for url in urls.iter().take(4) {
        if cancelled() {
            return Err(cancelled_error());
        }
        match download(client, url, referer, target, cancelled, progress).await {
            Ok(size) => return Ok(size),
            Err(error) if error.code == "VIDEO_CANCELLED" => return Err(error),
            Err(error) => {
                last = Some(error);
            }
        }
    }
    Err(last.unwrap_or_else(|| CommandError::new("VIDEO_NO_MEDIA", "没有可用的下载地址")))
}

/// 错误固定文本，不包含 CDN 签名查询或本地目标路径。
fn failed(message: &str) -> CommandError {
    CommandError::new("VIDEO_DOWNLOAD_FAILED", message)
}

/// 所有取消分支共享稳定错误码供任务层识别。
fn cancelled_error() -> CommandError {
    CommandError::new("VIDEO_CANCELLED", "已停止视频任务")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    /// 已取消任务不得开始网络请求，备用地址不能掩盖取消。
    async fn cancelled_before_request_wins() {
        let client = BiliClient::new(super::super::http::CookieJar::default()).unwrap();
        let mut progress = |_, _| panic!("取消任务不应写入");
        let error = download_first(
            &client,
            &["https://invalid.test/".into()],
            "",
            Path::new("unused"),
            &|| true,
            &mut progress,
        )
        .await
        .unwrap_err();
        assert_eq!(error.code, "VIDEO_CANCELLED");
        let error = download(
            &client,
            "https://invalid.test/",
            "",
            Path::new("unused"),
            &|| true,
            &mut progress,
        )
        .await
        .unwrap_err();
        assert_eq!(error.code, "VIDEO_CANCELLED");
    }

    #[tokio::test]
    /// 即使工作 future 永不就绪，取消观察也必须有界结束。
    async fn cancellation_interrupts_stalled_work() {
        let cancel = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let observed = cancel.clone();
        let check = move || observed.load(std::sync::atomic::Ordering::Relaxed);
        let trigger = async {
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
            cancel.store(true, std::sync::atomic::Ordering::Relaxed);
        };
        let wait = async {
            tokio::select! {
                _ = observe_cancel(&check) => {},
                _ = std::future::pending::<()>() => panic!("挂起任务不应完成"),
            }
        };
        tokio::time::timeout(std::time::Duration::from_secs(1), async {
            tokio::join!(trigger, wait);
        })
        .await
        .unwrap();
    }
}
