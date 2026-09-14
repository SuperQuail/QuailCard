//! 轻量选中结果与有界原字节校验，不重新抽取模型未见过的画面。
use super::{CandidateFrame, CommandError, PathBuf};
use sha2::{Digest, Sha256};
use std::{io::Read, path::Path};

pub(super) const MAX_IMAGE_BYTES: usize = 12 * 1024 * 1024;

/// 队列仅保留临时文件身份，不积累候选 bytes 或 Base64。
pub(crate) struct SelectedFrame {
    pub seconds: f64,
    pub stamp: String,
    pub path: PathBuf,
    pub hash: [u8; 32],
    pub len: u64,
    pub(in crate::services::video_pipeline) file_name: String,
}

/// 只保护本次受管路径，异常或取消返回时清理不完整候选。
pub(super) struct PendingFile(pub Option<PathBuf>);
impl Drop for PendingFile {
    /// 媒体端口返回后才会销毁守卫，不与仍在运行的 FFmpeg 争抢文件。
    fn drop(&mut self) {
        if let Some(path) = &self.0 {
            let _ = std::fs::remove_file(path);
        }
    }
}

/// 摘要以模型实际看到的内存字节计算，不能用提交时文件冒充原候选。
pub(super) fn selected(candidate: CandidateFrame) -> SelectedFrame {
    SelectedFrame {
        seconds: candidate.seconds,
        stamp: candidate.stamp,
        path: candidate.path,
        hash: Sha256::digest(&candidate.bytes).into(),
        len: candidate.bytes.len() as u64,
        file_name: candidate.file_name,
    }
}

/// 先 stat 再分配，并限制读流长度，文件在 stat 后增长也不能突破预算。
pub(super) fn read_limited(path: &Path, max_bytes: usize) -> Result<Vec<u8>, CommandError> {
    let limit = max_bytes.min(MAX_IMAGE_BYTES) as u64;
    let metadata = std::fs::symlink_metadata(path)?;
    if !metadata.file_type().is_file() {
        return Err(changed());
    }
    if metadata.len() > limit {
        return Err(too_large());
    }
    let mut file = std::fs::File::open(path)?;
    let metadata = file.metadata()?;
    if !metadata.is_file() {
        return Err(changed());
    }
    if metadata.len() > limit {
        return Err(too_large());
    }
    let mut bytes = vec![0; metadata.len() as usize];
    file.read_exact(&mut bytes).map_err(|_| changed())?;
    if file.read(&mut [0u8; 1])? != 0 {
        return Err(changed());
    }
    Ok(bytes)
}

/// 调用者先用 VideoStorage 重新净化 expected，禁止任意路径或被修改内容进入附件。
pub(super) fn verify(selected: &SelectedFrame, expected: &Path) -> Result<Vec<u8>, CommandError> {
    if selected.path != expected || selected.len > MAX_IMAGE_BYTES as u64 {
        return Err(changed());
    }
    let bytes = read_limited(expected, selected.len as usize)?;
    let hash: [u8; 32] = Sha256::digest(&bytes).into();
    if bytes.len() as u64 != selected.len || hash != selected.hash {
        return Err(changed());
    }
    Ok(bytes)
}

/// 统一安全错误避免把 Vault 绝对路径带到前端。
fn changed() -> CommandError {
    CommandError::new(
        "VIDEO_FRAME_CHANGED",
        "候选图片身份或内容已变化，已跳过保存",
    )
}

/// 组预算与单图预算共用明确错误，调用者可以跳过该图而不发送超限数据。
fn too_large() -> CommandError {
    CommandError::new("VIDEO_FRAME_TOO_LARGE", "候选图片超过大小预算，已跳过")
}

#[cfg(test)]
mod tests {
    use super::*;

    struct TestDir(PathBuf);
    impl TestDir {
        /// 随机目录隔离并发测试，不引入额外依赖。
        fn new() -> Self {
            let path = std::env::temp_dir().join(format!("quail-frame-{}", uuid::Uuid::now_v7()));
            std::fs::create_dir_all(&path).unwrap();
            Self(path)
        }
        /// 只暴露当前测试拥有的临时根目录。
        fn path(&self) -> &Path {
            &self.0
        }
    }
    impl Drop for TestDir {
        /// 断言失败也回收测试文件，避免后续测试误用旧结果。
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    /// 同长度替换也必须通过哈希识别，而不能只比较路径和大小。
    #[test]
    fn rejects_changed_original_bytes() {
        let dir = TestDir::new();
        let path = dir.path().join("candidate.jpg");
        std::fs::write(&path, b"first").unwrap();
        let frame = selected(CandidateFrame {
            seconds: 1.0,
            stamp: "00:01".into(),
            file_name: "candidate".into(),
            path: path.clone(),
            bytes: b"first".to_vec(),
        });
        assert_eq!(verify(&frame, &path).unwrap(), b"first");
        std::fs::write(&path, b"other").unwrap();
        assert_eq!(
            verify(&frame, &path).unwrap_err().code,
            "VIDEO_FRAME_CHANGED"
        );
        assert!(verify(&frame, &dir.path().join("else.jpg")).is_err());
    }

    /// 稀疏文件超过 12 MiB 时在读取前被 stat 拒绝，也尊重剩余组额度。
    #[test]
    fn enforces_stat_and_group_limit() {
        let dir = TestDir::new();
        let path = dir.path().join("candidate.jpg");
        let file = std::fs::File::create(&path).unwrap();
        file.set_len(MAX_IMAGE_BYTES as u64 + 1).unwrap();
        drop(file);
        assert_eq!(
            read_limited(&path, usize::MAX).unwrap_err().code,
            "VIDEO_FRAME_TOO_LARGE"
        );
        std::fs::write(&path, b"12345").unwrap();
        assert_eq!(
            read_limited(&path, 4).unwrap_err().code,
            "VIDEO_FRAME_TOO_LARGE"
        );
        assert_eq!(read_limited(&path, 5).unwrap(), b"12345");
    }
}
