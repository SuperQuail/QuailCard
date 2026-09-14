use super::{generation_error, GenerationCallError, GenerationSession};
use crate::{
    card_generation::resolve_source,
    models::{CardSource, GenerationInput},
};
use serde::Deserialize;

#[derive(Clone, Debug, PartialEq)]
pub(super) struct ResolvedSource {
    pub excerpt: String,
    pub source: Option<CardSource>,
    pub unresolved_text: bool,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct SourceRange {
    start_line: usize,
    end_line: usize,
}

impl GenerationSession {
    /// 来源选择只能指向创建会话时固定的材料，不读取模型可修改的笔记。
    pub(super) fn resolve_item(
        &self,
        input: &GenerationInput,
        item: &serde_json::Value,
    ) -> Result<ResolvedSource, GenerationCallError> {
        let range = item.get("sourceRange").filter(|v| !v.is_null());
        let image = item.get("imageRef").filter(|v| !v.is_null());
        if range.is_some() && image.is_some() {
            return Err(generation_error(
                "INVALID_SOURCE",
                "sourceRange 与 imageRef 只能提供一个",
            ));
        }
        if let Some(range) = range {
            let range: SourceRange = serde_json::from_value(range.clone())
                .map_err(|_| generation_error("INVALID_SOURCE_RANGE", "行号必须是正整数"))?;
            let lines: Vec<&str> = self.snapshot.split('\n').collect();
            if range.start_line == 0
                || range.end_line < range.start_line
                || range.end_line > lines.len()
            {
                return Err(generation_error(
                    "INVALID_SOURCE_RANGE",
                    "行号超出固定材料范围或顺序无效",
                ));
            }
            let start = lines[..range.start_line - 1]
                .iter()
                .map(|s| s.len() + 1)
                .sum::<usize>();
            let end = start
                + lines[range.start_line - 1..range.end_line]
                    .iter()
                    .map(|s| s.len())
                    .sum::<usize>()
                + range.end_line
                - range.start_line;
            let excerpt = self.snapshot[start..end].to_string();
            if excerpt.chars().count() > 4_000 {
                return Err(generation_error(
                    "INVALID_SOURCE_RANGE",
                    "来源超过 4000 字符，请缩小 sourceRange",
                ));
            }
            if excerpt.trim().is_empty() {
                return Err(generation_error("INVALID_SOURCE", "来源不能只有空白"));
            }
            let scope = self.snapshot_scope.as_ref();
            let base = if self.document == self.snapshot {
                Some(0)
            } else {
                resolve_source(&self.document, &self.snapshot, scope).map(|s| s.from)
            };
            let source = base.and_then(|base| {
                let scope = CardSource {
                    from: base + self.snapshot[..start].encode_utf16().count(),
                    to: base + self.snapshot[..end].encode_utf16().count(),
                    excerpt: excerpt.clone(),
                    prefix: String::new(),
                    suffix: String::new(),
                };
                resolve_source(&self.document, &excerpt, Some(&scope))
            });
            return Ok(ResolvedSource {
                unresolved_text: source.is_none(),
                excerpt,
                source,
            });
        }
        if let Some(image) = image {
            let reference = image.as_str().ok_or_else(|| {
                generation_error("INVALID_IMAGE_REF", "imageRef 必须是图片 ID 或文件名")
            })?;
            let matches: Vec<_> = self
                .image_names
                .iter()
                .enumerate()
                .filter(|(i, name)| {
                    reference == format!("image-{}", i + 1) || reference == name.as_str()
                })
                .collect();
            if matches.len() != 1 {
                return Err(generation_error(
                    "INVALID_IMAGE_REF",
                    "图片引用必须唯一匹配已提供的 ID 或文件名",
                ));
            }
            return Ok(ResolvedSource {
                excerpt: matches[0].1.clone(),
                source: None,
                unresolved_text: false,
            });
        }
        self.resolve_legacy(
            input,
            item.get("source").and_then(|v| v.as_str()).unwrap_or(""),
        )
    }

    /// 历史逐字摘录入口继续容忍歧义，但不允许伪造来源。
    pub(super) fn resolve_legacy(
        &self,
        _input: &GenerationInput,
        source: &str,
    ) -> Result<ResolvedSource, GenerationCallError> {
        let excerpt = source.trim();
        if excerpt.is_empty() || excerpt.chars().count() > 4_000 {
            return Err(generation_error(
                "INVALID_SOURCE",
                "source 必须是 1-4000 字符的原文摘录或图片来源说明",
            ));
        }
        let in_text = self.snapshot.contains(excerpt);
        if !in_text && !self.image_names.iter().any(|name| excerpt.contains(name)) {
            return Err(generation_error(
                "SOURCE_NOT_FOUND",
                "source 摘录不存在于学习材料",
            ));
        }
        let scope = self.snapshot_scope.as_ref();
        let source = in_text
            .then(|| resolve_source(&self.document, excerpt, scope))
            .flatten();
        Ok(ResolvedSource {
            excerpt: excerpt.to_string(),
            unresolved_text: in_text && source.is_none(),
            source,
        })
    }
}

/// 编号只用于引用；JSON 字符串保留缩进、空行和 CRLF，不把编号混入摘录。
pub(super) fn numbered_generation_material(input: &GenerationInput) -> String {
    let lines: Vec<_> = input
        .source_text
        .split('\n')
        .enumerate()
        .map(|(i, text)| serde_json::json!({"number":i+1,"text":text}))
        .collect();
    let images: Vec<_> = input
        .images
        .iter()
        .enumerate()
        .map(|(i, image)| serde_json::json!({"imageId":format!("image-{}", i+1),"name":image.name}))
        .collect();
    serde_json::json!({"lines":lines,"images":images}).to_string()
}
