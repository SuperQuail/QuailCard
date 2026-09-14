//! 仅清除本版本生成的临时媒体，不猜测旧目录内容归属。
use super::paths;
use crate::error::CommandError;
use std::path::Path;

/// 调用方必须保证媒体进程已经结束；仅处理直接普通文件，不跟随链接或递归。
pub(super) fn task_media(root: &Path, task_id: &str) -> Result<(), CommandError> {
    for (relative, shots) in [
        (format!("tasks/{task_id}"), false),
        (format!("tasks/{task_id}/shots"), true),
    ] {
        let dir = paths::resolve(root, paths::CURRENT, &relative)?;
        let entries = match std::fs::read_dir(&dir) {
            Ok(entries) => entries,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
            Err(error) => return Err(error.into()),
        };
        for entry in entries {
            let entry = entry?;
            let name = entry.file_name();
            let Some(name) = name.to_str() else {
                continue;
            };
            if !generated_name(name, shots) || !entry.file_type()?.is_file() {
                continue;
            }
            let file = paths::resolve(root, paths::CURRENT, &format!("{relative}/{name}"))?;
            std::fs::remove_file(file)?;
        }
    }
    Ok(())
}

/// 删除整条任务：受控媒体、固定记录文件，最后只移除被清空的目录。
///
/// 未知文件一律保留并使其目录留存，不递归强删、不跟随链接，旧命名空间不参与。
pub(super) fn task_all(root: &Path, task_id: &str) -> Result<(), CommandError> {
    task_media(root, task_id)?;
    for name in [super::TASK_FILE, super::TRANSCRIPT_FILE] {
        let file = paths::resolve(root, paths::CURRENT, &format!("tasks/{task_id}/{name}"))?;
        remove_if_exists(&file)?;
    }
    for relative in [format!("tasks/{task_id}/shots"), format!("tasks/{task_id}")] {
        let dir = paths::resolve(root, paths::CURRENT, &relative)?;
        // 目录非空说明还有未知内容：保留目录，交回用户自行处理。
        match std::fs::remove_dir(&dir) {
            Ok(()) => {}
            Err(error)
                if matches!(
                    error.kind(),
                    std::io::ErrorKind::NotFound | std::io::ErrorKind::DirectoryNotEmpty
                ) => {}
            Err(error) => return Err(error.into()),
        }
    }
    Ok(())
}

/// 文件缺失视为已删除；其余错误必须上报，不能假装清理成功。
fn remove_if_exists(path: &Path) -> Result<(), CommandError> {
    match std::fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error.into()),
    }
}

/// 精确匹配流水线生成名称，任意扩展名和用户文件不得被顺带删除。
fn generated_name(name: &str, shots: bool) -> bool {
    if shots {
        return name
            .strip_prefix("shot-p")
            .and_then(|n| n.strip_suffix(".jpg"))
            .and_then(|n| n.split_once('-'))
            .is_some_and(|(page, stamp)| digits(page) && digits(stamp));
    }
    let stem = name
        .strip_suffix(".m4s")
        .and_then(|n| {
            n.strip_prefix("audio_p")
                .or_else(|| n.strip_prefix("video_p"))
        })
        .or_else(|| {
            name.strip_suffix(".wav")
                .and_then(|n| n.strip_prefix("audio_p"))
        });
    stem.is_some_and(digits)
}

/// 非空十进制编号才属于受控生成名称。
fn digits(value: &str) -> bool {
    !value.is_empty() && value.bytes().all(|b| b.is_ascii_digit())
}
