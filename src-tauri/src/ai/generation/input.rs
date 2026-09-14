use base64::{engine::general_purpose, Engine as _};

use super::profile::generation_profile;
use crate::{error::CommandError, models::GenerationInput};

/// 校验边界输入一次，后续提示词和工具构造仅消费已校验材料。
pub fn validate_generation_input(input: &GenerationInput) -> Result<(), CommandError> {
    let profile = generation_profile(&input.type_id)?;
    if !profile.study_modes.contains(&input.study_mode_id.as_str()) {
        return Err(CommandError::validation("卡片类型与生成方式不匹配"));
    }
    if input.note_title.trim().is_empty() || input.note_title.chars().count() > 200 {
        return Err(CommandError::validation("笔记名称长度必须为 1-200 个字符"));
    }
    if input.source_text.trim().is_empty() && input.images.is_empty() {
        return Err(CommandError::validation("请提供文字或图片学习材料"));
    }
    if input.source_text.chars().count() > 200_000 {
        return Err(CommandError::validation("学习材料不能超过 200,000 个字符"));
    }
    if input.requested_count != -1 && !(1..=30).contains(&input.requested_count) {
        return Err(CommandError::validation("卡片数量必须为 AI 决定或 1-30"));
    }
    validate_images(input)
}

/// 实际解码校验大小，防止仅凭 Base64 字符串长度放过超限输入。
fn validate_images(input: &GenerationInput) -> Result<(), CommandError> {
    if input.images.len() > 4 {
        return Err(CommandError::validation("一次最多发送 4 张图片"));
    }
    let mut total = 0_usize;
    for image in &input.images {
        if image.name.trim().is_empty() || image.name.chars().count() > 255 {
            return Err(CommandError::validation("图片文件名无效"));
        }
        if !["image/png", "image/jpeg", "image/webp"].contains(&image.mime_type.as_str()) {
            return Err(CommandError::validation("仅支持 PNG、JPG 和 WebP 图片"));
        }
        let bytes = general_purpose::STANDARD
            .decode(&image.data_base64)
            .map_err(|_| CommandError::validation("图片数据不是有效 Base64"))?;
        if bytes.is_empty() || bytes.len() > 5 * 1024 * 1024 {
            return Err(CommandError::validation("单张图片大小必须在 5 MiB 以内"));
        }
        total = total.saturating_add(bytes.len());
    }
    if total > 15 * 1024 * 1024 {
        return Err(CommandError::validation("图片总大小不能超过 15 MiB"));
    }
    Ok(())
}
