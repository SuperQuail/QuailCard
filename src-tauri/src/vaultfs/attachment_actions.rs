//! 图片附件动作：读取校验、另存写入与对话框提示信息。
//!
//! 只处理 Vault 内图片的读取与用户选定目标的写入；
//! 剪贴板在 image_clipboard，系统对话框与文件管理器调用在命令层。

use std::{io::Write, path::Path};

use uuid::Uuid;

use super::attachments::{detect_image, extension_for_mime, file_error, MAX_IMAGE_BYTES};
use crate::error::CommandError;

/// 读取并校验附件字节：大小在读取前后各校验一次，签名由调用方按需检查。
///
/// 展示、复制与另存共用同一入口，避免新增动作放松既有约束。
pub(crate) fn read_attachment_bytes(path: &Path) -> Result<Vec<u8>, CommandError> {
    let metadata =
        std::fs::metadata(path).map_err(|error| file_error("读取图片附件失败", error))?;
    if !metadata.file_type().is_file() {
        return Err(CommandError::new(
            "ATTACHMENT_NOT_FOUND",
            "图片附件不存在或不可访问",
        ));
    }
    if metadata.len() > MAX_IMAGE_BYTES as u64 {
        return Err(CommandError::validation("图片不能超过 10 MiB"));
    }
    let bytes = std::fs::read(path).map_err(|error| file_error("读取图片附件失败", error))?;
    if bytes.len() > MAX_IMAGE_BYTES {
        return Err(CommandError::validation("图片不能超过 10 MiB"));
    }
    Ok(bytes)
}

/// 另存对话框的默认文件名与扩展名：扩展名按真实图片类型给出，不信任文件名。
pub(crate) fn attachment_file_hints(
    path: &Path,
    bytes: &[u8],
) -> Result<(String, &'static str), CommandError> {
    let mime = detect_image(bytes)?;
    let stem = path
        .file_stem()
        .map(|value| value.to_string_lossy().into_owned())
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| "image".to_string());
    Ok((stem, extension_for_mime(mime)))
}

/// 原子写入另存目标：同目录临时文件 + rename，失败时清理临时文件。
///
/// 目标可能已存在（用户在对话框里确认过覆盖），因此先写完整临时文件再替换，
/// 中途失败不会留下半截图片。
pub(crate) fn write_replacing(target: &Path, bytes: &[u8]) -> Result<(), CommandError> {
    let parent = target
        .parent()
        .filter(|parent| parent.is_dir())
        .ok_or_else(|| CommandError::validation("另存位置不存在"))?;
    let temp = parent.join(format!(".quailcard-save-{}", Uuid::now_v7()));
    let written = write_temp(&temp, bytes).and_then(|()| std::fs::rename(&temp, target));
    if let Err(error) = written {
        let _ = std::fs::remove_file(&temp);
        eprintln!("SAVE_ATTACHMENT_ERROR(detail): {error}");
        return Err(CommandError::new("SAVE_ERROR", "保存图片失败"));
    }
    Ok(())
}

/// 写临时文件并落盘：rename 之前必须确保数据已经写全。
fn write_temp(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)?;
    file.write_all(bytes)?;
    file.sync_all()
}
