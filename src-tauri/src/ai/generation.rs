use std::collections::{HashMap, HashSet};

use serde::Deserialize;
use serde_json::Value;
use uuid::Uuid;

use crate::{
    card_generation::card_identity,
    models::{GeneratedCard, GenerationInput, GenerationResult, NoteCard},
};

mod input;
mod planning;
mod profile;
mod sources;
use planning::PlanItem;
mod tools;
pub use input::validate_generation_input;
pub use tools::{build_generation_prompt, generation_mode_prompt, generation_tools};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawGenerationCard {
    schema_version: u32,
    type_id: String,
    fields: HashMap<String, String>,
    #[serde(default)]
    source: String,
    #[serde(default, rename = "itemId")]
    _item_id: Option<String>,
}

/// 单次调用错误仅包含可安全反馈给模型的诊断。
#[derive(Debug)]
pub struct GenerationCallError {
    pub code: &'static str,
    pub message: String,
}

/// 已校验草稿与笔记既有身份共同组成当前会话的去重边界。
pub struct GenerationSession {
    cards: Vec<GeneratedCard>,
    identities: HashSet<String>,
    /// 稳定 ID 的增量清单保留失败占位与已完成状态。
    plan: Vec<PlanItem>,
    plan_high_water: usize,
    /// None 表示数量不设上限，由模型调用 finish_generation 结束。
    target: Option<usize>,
    document: String,
    snapshot: String,
    snapshot_scope: Option<crate::models::CardSource>,
    image_names: Vec<String>,
    warnings: Vec<String>,
}

impl GenerationSession {
    /// 兼容无笔记上下文的纯生成入口，图片在此仅验证一次。
    #[cfg(test)]
    pub fn new(input: &GenerationInput) -> Result<Self, crate::error::CommandError> {
        validate_generation_input(input)?;
        Ok(Self::prepared(input, input.source_text.clone(), &[]))
    }

    /// 已完成输入和磁盘快照校验后创建会话，避免重复解码图片。
    pub fn prepared(input: &GenerationInput, document: String, existing: &[NoteCard]) -> Self {
        let identities = existing
            .iter()
            .filter(|card| card.kind == input.type_id)
            .map(|card| card_identity(&card.kind, &card.front, &card.back))
            .collect();
        Self {
            cards: Vec::new(),
            identities,
            plan: Vec::new(),
            plan_high_water: 0,
            document,
            snapshot: input.source_text.clone(),
            snapshot_scope: input.context.as_ref().and_then(|c| c.selection.clone()),
            image_names: input
                .images
                .iter()
                .map(|image| image.name.clone())
                .collect(),
            warnings: Vec::new(),
            target: if input.requested_count == -1 {
                None
            } else {
                Some(input.requested_count as usize)
            },
        }
    }

    /// 模型每次只提交一张卡，来源或字段无效不会污染已接受草稿。
    pub fn accept(
        &mut self,
        input: &GenerationInput,
        arguments: Value,
    ) -> Result<(), GenerationCallError> {
        if self.fixed_complete() {
            return Err(generation_error(
                "COUNT_LIMIT_REACHED",
                "已达到卡片数量上限",
            ));
        }
        let slot = if arguments.get("itemId").is_some() {
            Some(self.prepare_emit(arguments.clone())?.0)
        } else {
            None
        };
        let raw: RawGenerationCard = serde_json::from_value(arguments).map_err(|_| {
            generation_error(
                "INVALID_SCHEMA",
                "卡片工具参数不符合约定结构，必须提供 source 原文摘录",
            )
        })?;
        if raw.schema_version != 1 || raw.type_id != input.type_id {
            return Err(generation_error(
                "TYPE_MISMATCH",
                "卡片版本或类型与请求不匹配",
            ));
        }
        let profile = profile::generation_profile(&input.type_id)
            .map_err(|_| generation_error("INVALID_SCHEMA", "卡片类型未注册"))?;
        let mut fields = profile.validate_fields(&input.study_mode_id, raw.fields)?;
        let resolved = if let Some(index) = slot {
            self.plan[index]
                .resolved
                .clone()
                .expect("validated plan item")
        } else {
            self.resolve_legacy(input, &raw.source)?
        };
        let source = resolved.source;
        let unresolved_text = resolved.unresolved_text;
        fields.insert("source".to_string(), resolved.excerpt);
        let identity = card_identity(&input.type_id, &fields["front"], &fields["back"]);
        if !self.identities.insert(identity) {
            return Err(generation_error(
                "DUPLICATE_CARD",
                "卡片与当前笔记或本轮已接收内容重复",
            ));
        }
        self.cards.push(GeneratedCard {
            draft_id: Uuid::now_v7().to_string(),
            fields,
            source,
        });
        if unresolved_text && !self.warnings.iter().any(|warning| warning.contains("多处")) {
            self.warnings
                .push("部分来源摘录在原文多处出现，已保留摘录但未建立位置关联".to_string());
        }
        if let Some(index) = slot {
            self.mark_plan_emitted(index);
        }
        Ok(())
    }

    /// 对外暴露有效数量，进度不受失败或重复工具调用影响。
    pub fn generated(&self) -> usize {
        self.cards.len()
    }

    /// 上限模式返回剩余张数；不限量返回 None，模型据材料自行结束。
    pub fn remaining(&self) -> Option<usize> {
        self.target
            .map(|target| target.saturating_sub(self.cards.len()))
    }

    /// 只有显式数量上限到达时才终止；不限量由 finish_generation 结束。
    pub fn fixed_complete(&self) -> bool {
        self.target.is_some_and(|target| self.cards.len() >= target)
    }

    /// 将有效草稿及累计来源提示完整返回，不自动丢弃部分结果。
    pub fn finish(mut self, warning: Option<String>) -> GenerationResult {
        self.warnings.extend(warning);
        GenerationResult {
            cards: self.cards,
            warnings: self.warnings,
        }
    }
}

/// 统一安全错误结构，禁止携带模型原始载荷。
fn generation_error(code: &'static str, message: impl Into<String>) -> GenerationCallError {
    GenerationCallError {
        code,
        message: message.into(),
    }
}

#[cfg(test)]
#[path = "generation/range_tests.rs"]
mod range_tests;
#[cfg(test)]
#[path = "generation_tests.rs"]
mod tests;
