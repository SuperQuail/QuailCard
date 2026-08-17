use std::{
    fs::OpenOptions,
    io::Write,
    path::{Component, Path, PathBuf},
};

use base64::{engine::general_purpose, Engine as _};
use uuid::Uuid;

use super::config::normalize_attachment_folder;
use super::{sanitize_relative, VaultState};
use crate::{
    error::CommandError,
    models::{
        AttachmentImage, ImportNoteAttachmentInput, ImportedAttachment, ReadNoteAttachmentInput,
    },
};

const MAX_IMAGE_BYTES: usize = 10 * 1024 * 1024;

impl VaultState {
    /// 验证图片载荷并以 UUID v7 唯一文件名导入配置目录。
    pub fn import_note_attachment(
        &self,
        input: ImportNoteAttachmentInput,
    ) -> Result<ImportedAttachment, CommandError> {
        let root = self.require_root()?;
        let note_relative = sanitize_relative(&input.note_path)?;
        let note = root.join(&note_relative);
        let note = note
            .canonicalize()
            .map_err(|_| CommandError::new("NOTE_NOT_FOUND", "笔记文件不存在"))?;
        ensure_contained(&root, &note)?;
        if !note.is_file() {
            return Err(CommandError::new("NOTE_NOT_FOUND", "笔记文件不存在"));
        }
        let bytes = decode_image(&input.data_base64)?;
        let actual_mime = detect_image(&bytes)?;
        if normalize_mime(&input.mime_type)? != actual_mime {
            return Err(CommandError::validation("图片类型与文件内容不匹配"));
        }
        let stem = safe_file_stem(&input.file_name)?;
        let extension = extension_for_mime(actual_mime);
        let folder = normalize_attachment_folder(&self.get_config()?.attachment_folder)?;
        let directory = create_contained_directories(&root, &folder)?;
        let file_name = format!("{stem}-{}.{}", Uuid::now_v7(), extension);
        let target = directory.join(&file_name);
        write_new_file(&target, &bytes, "保存图片附件失败")?;
        let target_relative = Path::new(&folder).join(file_name);
        Ok(ImportedAttachment {
            markdown_path: relative_markdown_path(
                note_relative.parent().unwrap_or_else(|| Path::new("")),
                &target_relative,
            ),
        })
    }

    /// 解析任意旧 Markdown 图片路径，并仅返回 Vault 内的已验证图片。
    pub fn read_note_attachment(
        &self,
        input: ReadNoteAttachmentInput,
    ) -> Result<AttachmentImage, CommandError> {
        let root = self.require_root()?;
        let note_relative = sanitize_relative(&input.note_path)?;
        let note = root
            .join(&note_relative)
            .canonicalize()
            .map_err(|_| CommandError::new("NOTE_NOT_FOUND", "笔记文件不存在"))?;
        ensure_contained(&root, &note)?;
        if !note.is_file() {
            return Err(CommandError::new("NOTE_NOT_FOUND", "笔记文件不存在"));
        }
        let target_relative = resolve_markdown_source(
            note_relative.parent().unwrap_or_else(|| Path::new("")),
            &input.source,
        )?;
        let target = root
            .join(target_relative)
            .canonicalize()
            .map_err(|_| CommandError::new("ATTACHMENT_NOT_FOUND", "图片附件不存在或不可访问"))?;
        ensure_contained(&root, &target)?;
        let metadata =
            std::fs::metadata(&target).map_err(|error| file_error("读取图片附件失败", error))?;
        if !metadata.file_type().is_file() {
            return Err(CommandError::new(
                "ATTACHMENT_NOT_FOUND",
                "图片附件不存在或不可访问",
            ));
        }
        if metadata.len() > MAX_IMAGE_BYTES as u64 {
            return Err(CommandError::validation("图片不能超过 10 MiB"));
        }
        let bytes = std::fs::read(target).map_err(|error| file_error("读取图片附件失败", error))?;
        if bytes.len() > MAX_IMAGE_BYTES {
            return Err(CommandError::validation("图片不能超过 10 MiB"));
        }
        let mime_type = detect_image(&bytes)?.to_string();
        Ok(AttachmentImage {
            mime_type,
            data_base64: general_purpose::STANDARD.encode(bytes),
        })
    }

    /// 获取已选择且规范化过的 Vault 根目录。
    pub(super) fn require_root(&self) -> Result<PathBuf, CommandError> {
        self.root()?
            .ok_or_else(|| CommandError::new("VAULT_NOT_OPEN", "请先选择 Vault 文件夹"))
    }
}

/// Base64 输入拒绝空白和超限编码，避免解码前的异常分配。
fn decode_image(encoded: &str) -> Result<Vec<u8>, CommandError> {
    let max_encoded = MAX_IMAGE_BYTES.div_ceil(3) * 4;
    if encoded.is_empty() || encoded.len() > max_encoded || encoded.chars().any(char::is_whitespace)
    {
        return Err(CommandError::validation("图片 Base64 数据无效或过大"));
    }
    let bytes = general_purpose::STANDARD
        .decode(encoded)
        .map_err(|_| CommandError::validation("图片 Base64 数据无效"))?;
    if bytes.len() > MAX_IMAGE_BYTES {
        return Err(CommandError::validation("图片不能超过 10 MiB"));
    }
    Ok(bytes)
}

/// 通过文件签名识别受支持图片，不信任扩展名或声明类型。
fn detect_image(bytes: &[u8]) -> Result<&'static str, CommandError> {
    if bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
        Ok("image/png")
    } else if bytes.starts_with(&[0xff, 0xd8, 0xff]) {
        Ok("image/jpeg")
    } else if bytes.len() >= 12 && &bytes[..4] == b"RIFF" && &bytes[8..12] == b"WEBP" {
        Ok("image/webp")
    } else {
        Err(CommandError::validation(
            "仅支持有效的 PNG、JPEG 或 WebP 图片",
        ))
    }
}

/// 将允许的 MIME 声明规范为稳定 wire 值。
fn normalize_mime(mime: &str) -> Result<&'static str, CommandError> {
    match mime.trim().to_ascii_lowercase().as_str() {
        "image/png" => Ok("image/png"),
        "image/jpeg" | "image/jpg" => Ok("image/jpeg"),
        "image/webp" => Ok("image/webp"),
        _ => Err(CommandError::validation("仅支持 PNG、JPEG 或 WebP 图片")),
    }
}

/// MIME 类型决定托管文件扩展名，避免用户文件名伪装类型。
fn extension_for_mime(mime: &str) -> &'static str {
    match mime {
        "image/png" => "png",
        "image/jpeg" => "jpg",
        _ => "webp",
    }
}

/// 从用户文件名提取严格安全且长度受限的显示词干。
fn safe_file_stem(file_name: &str) -> Result<String, CommandError> {
    let name = file_name.trim();
    if name.is_empty() || name.len() > 200 || name.contains(['/', '\\']) {
        return Err(CommandError::validation("图片文件名无效"));
    }
    let stem = name.rsplit_once('.').map_or(name, |(stem, _)| stem).trim();
    if stem.is_empty()
        || stem == "."
        || stem == ".."
        || stem.chars().any(|character| {
            character.is_control() || matches!(character, '<' | '>' | ':' | '"' | '|' | '?' | '*')
        })
    {
        return Err(CommandError::validation("图片文件名无效"));
    }
    let mut safe = String::new();
    let mut separator = false;
    for character in stem.chars().take(80) {
        if character.is_alphanumeric() || matches!(character, '-' | '_') {
            safe.push(character);
            separator = false;
        } else if !separator && !safe.is_empty() {
            safe.push('-');
            separator = true;
        }
    }
    let safe = safe.trim_end_matches('-').to_string();
    if safe.is_empty() {
        return Err(CommandError::validation("图片文件名无效"));
    }
    Ok(safe)
}

/// 逐层创建附件目录，并在每层检查符号链接后的真实位置。
fn create_contained_directories(root: &Path, folder: &str) -> Result<PathBuf, CommandError> {
    let mut directory = root.to_path_buf();
    for segment in folder.split('/') {
        directory.push(segment);
        if !directory.exists() {
            std::fs::create_dir(&directory)
                .map_err(|error| file_error("创建附件目录失败", error))?;
        }
        let canonical = directory
            .canonicalize()
            .map_err(|error| file_error("创建附件目录失败", error))?;
        ensure_contained(root, &canonical)?;
        if !canonical.is_dir() {
            return Err(CommandError::validation("附件目录无效"));
        }
        directory = canonical;
    }
    Ok(directory)
}

/// Markdown 来源允许向上寻址，但词法归一化不得越过 Vault 根。
fn resolve_markdown_source(note_parent: &Path, source: &str) -> Result<PathBuf, CommandError> {
    let source = source.trim();
    if source.is_empty()
        || source.contains(['\\', '?', '#', ':'])
        || source.starts_with('/')
        || source.starts_with("//")
    {
        return Err(CommandError::validation("图片来源必须是 Vault 内相对路径"));
    }
    let mut parts: Vec<_> = note_parent
        .components()
        .filter_map(|component| match component {
            Component::Normal(value) => Some(value.to_os_string()),
            _ => None,
        })
        .collect();
    for component in Path::new(source).components() {
        match component {
            Component::Normal(value) => parts.push(value.to_os_string()),
            Component::CurDir => {}
            Component::ParentDir if parts.pop().is_some() => {}
            _ => return Err(CommandError::validation("图片路径超出 Vault 范围")),
        }
    }
    if parts.is_empty() {
        return Err(CommandError::validation("图片来源无效"));
    }
    Ok(parts.into_iter().collect())
}

/// 生成从笔记父目录到托管文件的标准 `/` Markdown 路径。
fn relative_markdown_path(note_parent: &Path, target: &Path) -> String {
    let from: Vec<_> = note_parent.components().collect();
    let to: Vec<_> = target.components().collect();
    let common = from
        .iter()
        .zip(&to)
        .take_while(|(left, right)| left == right)
        .count();
    let mut parts = vec!["..".to_string(); from.len() - common];
    parts.extend(
        to[common..]
            .iter()
            .map(|component| component.as_os_str().to_string_lossy().into_owned()),
    );
    parts.join("/")
}

/// 新文件一律使用 create_new，确保导入与临时配置不会覆盖现有内容。
pub(super) fn write_new_file(
    path: &Path,
    bytes: &[u8],
    message: &'static str,
) -> Result<(), CommandError> {
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .map_err(|error| file_error(message, error))?;
    let result = file.write_all(bytes).and_then(|_| file.sync_all());
    drop(file);
    if let Err(error) = result {
        let _ = std::fs::remove_file(path);
        return Err(file_error(message, error));
    }
    Ok(())
}

/// 所有 canonical 路径必须仍位于当前 Vault 根目录之内。
pub(super) fn ensure_contained(root: &Path, path: &Path) -> Result<(), CommandError> {
    if path.starts_with(root) {
        Ok(())
    } else {
        Err(CommandError::validation("路径超出 Vault 范围"))
    }
}

/// 文件系统细节仅写后端日志，前端只收到安全错误。
pub(super) fn file_error(message: &'static str, error: std::io::Error) -> CommandError {
    eprintln!("ATTACHMENT_FILE_ERROR(detail): {error}");
    CommandError::new("FILE_ERROR", message)
}
