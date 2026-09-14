//! 全局串行模型下载与窗口级进度快照，避免共享 .part 文件和取消标记互相覆盖。
use crate::{error::CommandError, video::models::VideoDownloadStatus};
use std::{
    collections::HashMap,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex,
    },
    time::Instant,
};

#[derive(Clone)]
struct Download {
    status: VideoDownloadStatus,
    cancel: Arc<AtomicBool>,
    started: Instant,
}

#[derive(Default, Clone)]
pub(crate) struct VideoDownloads {
    entries: Arc<Mutex<HashMap<String, Download>>>,
}

pub(crate) struct DownloadHandle {
    registry: VideoDownloads,
    owner: String,
    cancel: Arc<AtomicBool>,
}

impl VideoDownloads {
    /// 下载写同一应用目录，任何窗口只允许一个下载，其他请求明确报忙。
    pub(crate) fn begin(
        &self,
        owner: &str,
        model_id: &str,
    ) -> Result<DownloadHandle, CommandError> {
        let mut entries = self.entries.lock().unwrap_or_else(|e| e.into_inner());
        if entries
            .values()
            .any(|item| item.status.state == "downloading")
        {
            return Err(CommandError::new(
                "VIDEO_DOWNLOAD_BUSY",
                "已有模型正在下载，请等待完成或取消后重试",
            ));
        }
        let cancel = Arc::new(AtomicBool::new(false));
        entries.insert(
            owner.to_string(),
            Download {
                status: VideoDownloadStatus {
                    model_id: model_id.to_string(),
                    state: "downloading".into(),
                    ..Default::default()
                },
                cancel: cancel.clone(),
                started: Instant::now(),
            },
        );
        Ok(DownloadHandle {
            registry: self.clone(),
            owner: owner.to_string(),
            cancel,
        })
    }

    /// 快照读不产生下载；终态保留到下一次下载，便于弹窗重开显示结果。
    pub(crate) fn status(&self, owner: &str) -> VideoDownloadStatus {
        self.entries
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .get(owner)
            .map(|item| item.status.clone())
            .unwrap_or_else(|| VideoDownloadStatus {
                state: "idle".into(),
                ..Default::default()
            })
    }

    /// 只取消该窗口的下载，不误伤其他窗口。
    pub(crate) fn cancel(&self, owner: &str) {
        if let Some(item) = self
            .entries
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .get(owner)
        {
            item.cancel.store(true, Ordering::SeqCst);
        }
    }
}

impl DownloadHandle {
    /// 供流式下载在等待数据期间主动检测取消。
    pub(crate) fn is_cancelled(&self) -> bool {
        self.cancel.load(Ordering::SeqCst)
    }

    /// 字节和平均速度来自实际下载回调，不按时间伪造进度。
    pub(crate) fn progress(&self, written: u64, total: Option<u64>) {
        if let Some(item) = self
            .registry
            .entries
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .get_mut(&self.owner)
        {
            if !Arc::ptr_eq(&item.cancel, &self.cancel) {
                return;
            }
            item.status.downloaded_bytes = written;
            item.status.total_bytes = total;
            item.status.bytes_per_second =
                (written as f64 / item.started.elapsed().as_secs_f64().max(0.001)) as u64;
        }
    }

    /// 成功、失败和取消都释放全局下载占用，并保留安全状态信息。
    pub(crate) fn finish(&self, error: Option<&CommandError>) {
        if let Some(item) = self
            .registry
            .entries
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .get_mut(&self.owner)
        {
            if !Arc::ptr_eq(&item.cancel, &self.cancel) {
                return;
            }
            item.status.state = match error {
                None => "ready",
                Some(error) if error.code == "VIDEO_CANCELLED" => "cancelled",
                Some(_) => "failed",
            }
            .to_string();
            item.status.error = error.map(|error| error.message.clone());
            item.status.bytes_per_second = 0;
        }
    }
}

impl Drop for DownloadHandle {
    /// 请求意外退出仍释放占用，不让窗口一直处于下载中。
    fn drop(&mut self) {
        let mut entries = self
            .registry
            .entries
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        if let Some(item) = entries.get_mut(&self.owner) {
            if !Arc::ptr_eq(&item.cancel, &self.cancel) {
                return;
            }
            if item.status.state == "downloading" {
                item.status.state = "cancelled".into();
                item.status.error = Some("下载已中断，可重新下载".into());
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    /// 同窗口和跨窗口均不能覆盖正在执行的下载，完成后可开始新的下载。
    fn serializes_downloads_and_retains_progress() {
        let registry = VideoDownloads::default();
        let handle = registry.begin("main", "small").unwrap();
        assert!(registry.begin("main", "base").is_err());
        assert!(registry.begin("other", "small").is_err());
        handle.progress(10, Some(20));
        assert_eq!(registry.status("main").downloaded_bytes, 10);
        registry.cancel("other");
        assert!(!handle.is_cancelled());
        registry.cancel("main");
        assert!(handle.is_cancelled());
        handle.finish(Some(&CommandError::new("VIDEO_CANCELLED", "已取消")));
        assert_eq!(registry.status("main").state, "cancelled");
        drop(handle);
        let next = registry.begin("other", "base").unwrap();
        next.finish(None);
        assert_eq!(registry.status("other").state, "ready");
    }

    #[test]
    /// 已结束句柄迟到的进度或析构不能修改同窗口后续下载。
    fn stale_handle_cannot_cancel_new_download() {
        let registry = VideoDownloads::default();
        let old = registry.begin("main", "small").unwrap();
        old.finish(None);
        let new = registry.begin("main", "base").unwrap();
        old.progress(100, Some(100));
        old.finish(Some(&CommandError::new("VIDEO_CANCELLED", "旧请求")));
        drop(old);
        assert_eq!(registry.status("main").state, "downloading");
        assert_eq!(registry.status("main").downloaded_bytes, 0);
        new.finish(None);
    }

    #[test]
    /// 未正常收尾的请求退出后也能再次下载。
    fn abandoned_handle_releases_slot() {
        let registry = VideoDownloads::default();
        drop(registry.begin("main", "small").unwrap());
        assert_eq!(registry.status("main").state, "cancelled");
        assert!(registry.begin("main", "small").is_ok());
    }
}
