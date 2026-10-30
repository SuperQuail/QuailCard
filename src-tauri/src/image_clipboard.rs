//! 图片剪贴板：把附件字节解码成 RGBA 位图后写入系统剪贴板。
//!
//! 只负责解码与剪贴板写入，不解析 Vault 路径，便于单独验证解码边界。

use std::{borrow::Cow, io::Cursor};

use arboard::{Clipboard, ImageData};
use image::{ImageReader, Limits};

use crate::error::CommandError;

/// 位图像素上限：解码后的 RGBA 内存可达压缩数据的上百倍，超限直接拒绝。
const MAX_PIXELS: u64 = 40_000_000;
/// 解码分配上限（字节）：畸形图片声明超大尺寸时由解码器兜底拒绝。
const MAX_DECODE_ALLOC: u64 = 256 * 1024 * 1024;
/// 单边像素上限：与像素总量一起约束，避免先分配再检查。
const MAX_SIDE: u32 = 20_000;

/// 解码后的 RGBA 位图，字段与 arboard 的 ImageData 保持一致。
#[derive(Debug)]
pub struct DecodedImage {
    pub width: usize,
    pub height: usize,
    pub rgba: Vec<u8>,
}

/// 按文件签名解码 PNG/JPEG/WebP 为 RGBA；像素或内存超限时返回可读错误。
pub fn decode_rgba(bytes: &[u8]) -> Result<DecodedImage, CommandError> {
    let mut reader = ImageReader::new(Cursor::new(bytes))
        .with_guessed_format()
        .map_err(|_| decode_failed())?;
    reader.limits(decode_limits());
    let decoded = reader.decode().map_err(|error| {
        eprintln!("IMAGE_DECODE_ERROR(detail): {error}");
        decode_failed()
    })?;
    let (width, height) = (decoded.width(), decoded.height());
    check_pixel_budget(width, height)?;
    Ok(DecodedImage {
        width: width as usize,
        height: height as usize,
        rgba: decoded.into_rgba8().into_raw(),
    })
}

/// 把解码结果写入系统剪贴板；剪贴板被其他程序占用时提示稍后重试。
pub fn copy_to_clipboard(image: DecodedImage) -> Result<(), CommandError> {
    let mut clipboard = Clipboard::new().map_err(clipboard_error)?;
    clipboard
        .set_image(ImageData {
            width: image.width,
            height: image.height,
            bytes: Cow::Owned(image.rgba),
        })
        .map_err(clipboard_error)
}

/// 解码上限：单边与总分配都受限，畸形图片无法撑爆内存。
fn decode_limits() -> Limits {
    let mut limits = Limits::default();
    limits.max_image_width = Some(MAX_SIDE);
    limits.max_image_height = Some(MAX_SIDE);
    limits.max_alloc = Some(MAX_DECODE_ALLOC);
    limits
}

/// 像素总量单独的硬上限：解码器只能约束单边，总量必须自己再检查。
fn check_pixel_budget(width: u32, height: u32) -> Result<(), CommandError> {
    if width == 0 || height == 0 {
        return Err(decode_failed());
    }
    if u64::from(width) * u64::from(height) > MAX_PIXELS {
        return Err(CommandError::validation("图片像素过大，无法复制到剪贴板"));
    }
    Ok(())
}

/// 解码失败只写后端日志，前端拿固定错误码。
fn decode_failed() -> CommandError {
    CommandError::new("IMAGE_DECODE_ERROR", "图片无法解码，无法复制到剪贴板")
}

/// 剪贴板失败同样只记录细节，避免把系统错误原文抛给用户。
fn clipboard_error(error: arboard::Error) -> CommandError {
    eprintln!("CLIPBOARD_ERROR(detail): {error}");
    CommandError::new("CLIPBOARD_ERROR", "无法写入系统剪贴板，请稍后重试")
}

#[cfg(test)]
#[path = "image_clipboard_tests.rs"]
mod image_clipboard_tests;
