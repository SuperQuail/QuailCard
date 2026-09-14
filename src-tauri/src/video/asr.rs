//! Whisper 模型清单、状态与按需下载。

mod download;
pub(crate) mod whisper_cli;
pub(crate) use download::download;

use std::path::{Path, PathBuf};

/// 可选的 whisper.cpp GGML 模型档位。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct ModelOption {
    pub id: &'static str,
    pub label: &'static str,
    pub file_name: &'static str,
    /// 官方文件大小（字节），下载与复用均要求精确匹配。
    pub bytes: u64,
}

/// 模型清单；默认档位为 small。
pub(crate) const MODELS: [ModelOption; 4] = [
    ModelOption {
        id: "base",
        label: "base（142MB，最快）",
        file_name: "ggml-base.bin",
        bytes: 147_951_465,
    },
    ModelOption {
        id: "small",
        label: "small（466MB，推荐）",
        file_name: "ggml-small.bin",
        bytes: 487_601_967,
    },
    ModelOption {
        id: "medium",
        label: "medium（1.5GB，更准）",
        file_name: "ggml-medium.bin",
        bytes: 1_533_774_781,
    },
    ModelOption {
        id: "large-v3-turbo",
        label: "large-v3-turbo（1.6GB，最准）",
        file_name: "ggml-large-v3-turbo.bin",
        bytes: 1_621_879_991,
    },
];

/// 按 id 查找模型；未知 id 返回 None。
pub(crate) fn find(id: &str) -> Option<&'static ModelOption> {
    MODELS.iter().find(|option| option.id == id)
}

/// 模型文件落盘位置（应用数据目录下，跨知识库复用）。
pub(crate) fn model_file(data_dir: &Path, option: &ModelOption) -> PathBuf {
    data_dir.join("asr-models").join(option.file_name)
}

/// 模型状态：missing（未下载）/ ready（可用）/ invalid（损坏）。
pub(crate) fn status(data_dir: &Path, option: &ModelOption) -> &'static str {
    let path = model_file(data_dir, option);
    let Ok(metadata) = std::fs::metadata(&path) else {
        return "missing";
    };
    if !metadata.is_file() || metadata.len() != option.bytes || !has_ggml_magic(&path) {
        return "invalid";
    }
    "ready"
}

/// whisper.cpp 将 0x67676d6c 按小端写入，磁盘字节是 lmgg 而非字符串 ggml。
fn has_ggml_magic(path: &Path) -> bool {
    use std::io::Read;
    let Ok(mut file) = std::fs::File::open(path) else {
        return false;
    };
    let mut magic = [0u8; 4];
    file.read_exact(&mut magic).is_ok() && u32::from_le_bytes(magic) == 0x6767_6d6c
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    /// 清单包含默认档位且 id 唯一。
    fn catalog_contains_default() {
        assert!(find("small").is_some());
        assert!(find("unknown").is_none());
        let ids: Vec<&str> = MODELS.iter().map(|option| option.id).collect();
        for id in &ids {
            assert_eq!(ids.iter().filter(|item| *item == id).count(), 1);
        }
    }

    #[test]
    /// 使用小型模型夹具验证磁盘字节序，避免有效模型一直被误判为未下载。
    fn recognizes_little_endian_ggml_and_rejects_invalid_files() {
        let dir = std::env::temp_dir().join(format!("qc-model-{}", uuid::Uuid::now_v7()));
        let option = ModelOption {
            id: "test",
            label: "test",
            file_name: "test.bin",
            bytes: 16,
        };
        let path = model_file(&dir, &option);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        let mut payload = Vec::from(0x6767_6d6c_u32.to_le_bytes());
        payload.resize(16, 0);
        std::fs::write(&path, &payload).unwrap();
        assert_eq!(status(&dir, &option), "ready");
        payload.resize(15, 0);
        std::fs::write(&path, &payload).unwrap();
        assert_eq!(status(&dir, &option), "invalid");
        payload.resize(17, 0);
        std::fs::write(&path, &payload).unwrap();
        assert_eq!(status(&dir, &option), "invalid");
        payload.resize(16, 0);
        payload[..4].copy_from_slice(b"ggml");
        std::fs::write(&path, &payload).unwrap();
        assert_eq!(status(&dir, &option), "invalid");
        std::fs::write(&path, b"lm").unwrap();
        assert_eq!(status(&dir, &option), "invalid");
        std::fs::write(&path, b"<html>not a model</html>").unwrap();
        assert_eq!(status(&dir, &option), "invalid");
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    /// 空目录下状态为未下载。
    fn status_reports_missing() {
        let dir = std::env::temp_dir().join(format!("qc-model-{}", uuid::Uuid::now_v7()));
        assert_eq!(status(&dir, &MODELS[1]), "missing");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
