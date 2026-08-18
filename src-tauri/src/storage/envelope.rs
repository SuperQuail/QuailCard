//! 持久化文件信封：版本熔断、容错加载与原子写。
//!
//! 所有持久化文件首字段为 formatVersion。加载时先解析为无类型值校验版本，
//! 再反序列化为具体结构：未知字段忽略、缺失字段取默认，保证旧文件永远
//! 能被新代码读取；版本高于当前应用时熔断报错，防止旧程序覆写丢数据。

use std::path::Path;

use base64::{engine::general_purpose::STANDARD, Engine as _};
use serde::{de::DeserializeOwned, Deserialize, Deserializer, Serialize, Serializer};
use uuid::Uuid;

use crate::error::CommandError;

/// 当前应用支持的全部持久化文件信封版本。
pub(crate) const CURRENT_FORMAT_VERSION: u64 = 1;

/// 文件解析失败后的处置策略。
pub(crate) enum CorruptPolicy {
    /// 可再生数据（配置文件）：把损坏文件备份为 .corrupt-<时间戳>，
    /// 返回 None 由调用方重建默认内容。
    BackupAndRegenerate,
    /// 不可再生数据（卡片、保险库）：携带安全错误码直接失败，绝不自动重置。
    Reject {
        code: &'static str,
        message: &'static str,
    },
}

/// 读取 TOML 文件并按信封规则解析；文件不存在返回 None。
pub(crate) fn load_toml<T: DeserializeOwned>(
    path: &Path,
    policy: &CorruptPolicy,
) -> Result<Option<T>, CommandError> {
    let Some(bytes) = read_existing(path) else {
        return Ok(None);
    };
    let text = String::from_utf8_lossy(&bytes);
    let value: toml::Value = match toml::from_str(&text) {
        Ok(value) => value,
        Err(error) => return regenerate_or_reject(path, policy, &error.to_string()),
    };
    ensure_supported_version(toml_version(&value), path)?;
    match T::deserialize(value) {
        Ok(parsed) => Ok(Some(parsed)),
        Err(error) => regenerate_or_reject(path, policy, &error.to_string()),
    }
}

/// 读取 JSON 文件并按信封规则解析；文件不存在返回 None。
pub(crate) fn load_json<T: DeserializeOwned>(
    path: &Path,
    policy: &CorruptPolicy,
) -> Result<Option<T>, CommandError> {
    let Some(bytes) = read_existing(path) else {
        return Ok(None);
    };
    let value: serde_json::Value = match serde_json::from_slice(&bytes) {
        Ok(value) => value,
        Err(error) => return regenerate_or_reject(path, policy, &error.to_string()),
    };
    ensure_supported_version(json_version(&value), path)?;
    match T::deserialize(value) {
        Ok(parsed) => Ok(Some(parsed)),
        Err(error) => regenerate_or_reject(path, policy, &error.to_string()),
    }
}

/// 序列化为 TOML 并原子写入目标路径。
pub(crate) fn save_toml<T: Serialize>(path: &Path, value: &T) -> Result<(), CommandError> {
    let text = toml::to_string_pretty(value).map_err(|error| {
        eprintln!("SETTINGS_SERIALIZE_ERROR(detail): {error}");
        CommandError::new("SERIALIZATION_ERROR", "配置序列化失败")
    })?;
    write_atomic(path, text.as_bytes())
}

/// 序列化为换行友好的 JSON 并原子写入目标路径。
pub(crate) fn save_json<T: Serialize>(path: &Path, value: &T) -> Result<(), CommandError> {
    let bytes = serde_json::to_vec_pretty(value).map_err(|error| {
        eprintln!("DATA_SERIALIZE_ERROR(detail): {error}");
        CommandError::new("SERIALIZATION_ERROR", "数据序列化失败")
    })?;
    write_atomic(path, &bytes)
}

/// 文件存在则读出全部字节，否则返回 None。
fn read_existing(path: &Path) -> Option<Vec<u8>> {
    match std::fs::read(path) {
        Ok(bytes) => Some(bytes),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
        Err(error) => {
            eprintln!("FILE_READ_ERROR(detail): {error}");
            Some(Vec::new())
        }
    }
}

/// 按策略处置解析失败：可再生数据备份后返回 None，不可再生数据报错。
fn regenerate_or_reject<T>(
    path: &Path,
    policy: &CorruptPolicy,
    detail: &str,
) -> Result<Option<T>, CommandError> {
    match policy {
        CorruptPolicy::BackupAndRegenerate => {
            let backup = path.with_file_name(format!(
                "{}.corrupt-{}",
                path.file_name()
                    .map(|name| name.to_string_lossy())
                    .unwrap_or_default(),
                crate::storage::now_timestamp()
            ));
            eprintln!("FILE_CORRUPT_BACKUP(detail): {detail} -> {backup:?}");
            if let Err(error) = std::fs::rename(path, &backup) {
                eprintln!("FILE_CORRUPT_BACKUP_ERROR(detail): {error}");
                return Err(CommandError::new(
                    "FILE_ERROR",
                    "损坏文件备份失败，请手动处理后重试",
                ));
            }
            Ok(None)
        }
        CorruptPolicy::Reject { code, message } => {
            eprintln!("FILE_CORRUPT_REJECT(detail): {detail} path={path:?}");
            Err(CommandError::new(code, *message))
        }
    }
}

/// 从 TOML 值中读取信封版本，缺失按当前版本处理。
fn toml_version(value: &toml::Value) -> u64 {
    value
        .get("formatVersion")
        .and_then(|version| version.as_integer())
        .unwrap_or(CURRENT_FORMAT_VERSION as i64) as u64
}

/// 从 JSON 值中读取信封版本，缺失按当前版本处理。
fn json_version(value: &serde_json::Value) -> u64 {
    value
        .get("formatVersion")
        .and_then(|version| version.as_u64())
        .unwrap_or(CURRENT_FORMAT_VERSION)
}

/// 版本熔断：文件由更新版本应用创建时拒绝打开。
fn ensure_supported_version(version: u64, path: &Path) -> Result<(), CommandError> {
    if version > CURRENT_FORMAT_VERSION {
        eprintln!("FILE_FORMAT_NEWER(detail): version={version} path={path:?}");
        return Err(CommandError::new(
            "FILE_FORMAT_NEWER",
            "文件由更新版本的 QuailCard 创建，请先升级应用",
        ));
    }
    Ok(())
}

/// 原子写入：先写同目录唯一临时文件并落盘，再 rename 替换目标。
///
/// 崩溃时最多留下无害的 .tmp 残留，目标文件永远保持完整旧版或完整新版。
fn write_atomic(path: &Path, bytes: &[u8]) -> Result<(), CommandError> {
    use std::io::Write;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let file_name = path
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_default();
    let temporary = path.with_file_name(format!("{file_name}.{}.tmp", Uuid::now_v7().simple()));
    {
        let mut file = std::fs::File::create(&temporary)?;
        file.write_all(bytes)?;
        file.sync_all()?;
    }
    std::fs::rename(&temporary, path)?;
    Ok(())
}

/// 二进制字段的 base64 序列化辅助，用于 JSON 文件中的字节载荷。
pub(crate) mod base64_field {
    use super::*;

    /// 把字节编码为 base64 字符串，保持文件为可读文本。
    pub(crate) fn serialize<S: Serializer>(value: &[u8], serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&STANDARD.encode(value))
    }

    /// 把 base64 字符串解码回字节，非法内容按反序列化错误处理。
    pub(crate) fn deserialize<'de, D: Deserializer<'de>>(
        deserializer: D,
    ) -> Result<Vec<u8>, D::Error> {
        let text = String::deserialize(deserializer)?;
        STANDARD.decode(text).map_err(serde::de::Error::custom)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde::{Deserialize, Serialize};

    /// 信封测试用的最小结构。
    #[derive(Debug, PartialEq, Serialize, Deserialize)]
    #[serde(rename_all = "camelCase", default)]
    struct Sample {
        format_version: u64,
        #[serde(default)]
        name: String,
    }

    impl Default for Sample {
        fn default() -> Self {
            Self {
                format_version: CURRENT_FORMAT_VERSION,
                name: String::new(),
            }
        }
    }

    #[test]
    /// 未知字段被忽略、缺失字段取默认，旧文件始终可读。
    fn tolerant_parsing_ignores_unknown_fields() {
        let path = std::env::temp_dir().join(format!("qc-env-{}.json", Uuid::now_v7()));
        std::fs::write(&path, br#"{"formatVersion":1,"futureField":{"x":1}}"#).unwrap();
        let sample: Option<Sample> = load_json(
            &path,
            &CorruptPolicy::Reject {
                code: "TEST",
                message: "测试",
            },
        )
        .unwrap();
        assert_eq!(sample, Some(Sample::default()));
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    /// 版本高于当前应用时熔断拒绝打开。
    fn rejects_newer_format_version() {
        let path = std::env::temp_dir().join(format!("qc-env-{}.json", Uuid::now_v7()));
        std::fs::write(&path, br#"{"formatVersion":99}"#).unwrap();
        let error = load_json::<Sample>(&path, &CorruptPolicy::BackupAndRegenerate)
            .expect_err("高版本不应打开");
        assert_eq!(error.code, "FILE_FORMAT_NEWER");
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    /// 可再生文件损坏时备份原件并返回 None。
    fn backs_up_regenerable_corrupt_file() {
        let dir = std::env::temp_dir().join(format!("qc-env-{}", Uuid::now_v7()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("settings.toml");
        std::fs::write(&path, "not [ valid toml").unwrap();
        let result: Option<Sample> = load_toml(&path, &CorruptPolicy::BackupAndRegenerate).unwrap();
        assert!(result.is_none());
        assert!(!path.exists(), "损坏原件应已被改名备份");
        let backups: Vec<_> = std::fs::read_dir(&dir)
            .unwrap()
            .filter_map(|entry| entry.ok())
            .filter(|entry| entry.file_name().to_string_lossy().contains(".corrupt-"))
            .collect();
        assert_eq!(backups.len(), 1, "应存在一个备份文件");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    /// 原子写完整替换目标文件且不留临时残留。
    fn atomic_write_replaces_target() {
        let dir = std::env::temp_dir().join(format!("qc-env-{}", Uuid::now_v7()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("data.json");
        save_json(&path, &Sample::default()).unwrap();
        save_json(
            &path,
            &Sample {
                name: "二".into(),
                ..Sample::default()
            },
        )
        .unwrap();
        let loaded: Option<Sample> = load_json(
            &path,
            &CorruptPolicy::Reject {
                code: "TEST",
                message: "测试",
            },
        )
        .unwrap();
        assert_eq!(loaded.map(|sample| sample.name), Some("二".into()));
        let leftover: Vec<_> = std::fs::read_dir(&dir)
            .unwrap()
            .filter_map(|entry| entry.ok())
            .filter(|entry| entry.file_name().to_string_lossy().ends_with(".tmp"))
            .collect();
        assert!(leftover.is_empty(), "不应留下临时文件");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
