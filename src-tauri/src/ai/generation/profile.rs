use std::collections::HashMap;

use super::{generation_error, GenerationCallError};
use crate::error::CommandError;

pub(super) struct GenerationProfile {
    pub type_id: &'static str,
    pub study_modes: &'static [&'static str],
    pub instruction: &'static str,
    pub fields: &'static [&'static str],
    pub dictionary: bool,
}

pub(super) const VOCABULARY_FRONT_FORMAT: &str = "禁止使用“名词: xxx”“动词: xxx”等中文词性加冒号或中文冒号的写法。单个单词按词性分段，每段写“词性缩写. 含义1,含义2”，多个词性用“; ”分隔，例如“v. 说,讲话; n. 演讲”，缩写写法可直接照抄词典查询结果（如 a.、ad.）；多词构成的词组不加词性前缀，直接写“含义1,含义2”，例如词条“look up”填“查阅,抬头”";

const PROFILES: &[GenerationProfile] = &[
    GenerationProfile {
        type_id: "vocabulary", study_modes: &["dictation"], dictionary: true,
        instruction: "front=中文释义（用于根据释义默写），back=待默写词条，detail=纯音标（如 /spiːk/，不可包含词性），example=简短例句，aliases=可接受答案并用顿号分隔；有词典结果时优先采用真实音标与释义，无音标留空",
        fields: &["front", "back", "detail", "example", "aliases"],
    },
    GenerationProfile {
        type_id: "qa", study_modes: &["self-review", "ai-review"], dictionary: false,
        instruction: "front=可独立理解的明确问题，一卡一个考点，back=有材料依据的独立完整参考答案，detail=材料来源摘要",
        fields: &["front", "back", "detail"],
    },
];

impl GenerationProfile {
    /// 学习方式扩展只在字段规则处追加评分要求。
    pub fn fields(&self, mode: &str) -> Vec<&'static str> {
        let mut fields = self.fields.to_vec();
        if mode == "ai-review" {
            fields.push("rubric");
        }
        fields
    }

    /// 所有字段都有限长，必需正文和评分要点禁止空值。
    pub fn validate_fields(
        &self,
        mode: &str,
        fields: HashMap<String, String>,
    ) -> Result<HashMap<String, String>, GenerationCallError> {
        let expected = self.fields(mode);
        if fields.keys().any(|key| !expected.contains(&key.as_str())) {
            return Err(generation_error("INVALID_SCHEMA", "卡片包含未注册字段"));
        }
        for key in expected {
            let value = fields
                .get(key)
                .ok_or_else(|| generation_error("MISSING_FIELD", format!("缺少字段 {key}")))?;
            if ["front", "back", "rubric"].contains(&key) && value.trim().is_empty() {
                return Err(generation_error(
                    "MISSING_FIELD",
                    format!("字段 {key} 不能为空"),
                ));
            }
            let limit = if key == "front" {
                2_000
            } else if key == "back" {
                8_000
            } else {
                4_000
            };
            if value.chars().count() > limit {
                return Err(generation_error("FIELD_TOO_LONG", "卡片字段超过长度限制"));
            }
        }
        Ok(fields)
    }
}

/// 新卡片规则通过登记配置扩展，工具执行器无需识别类型字符串。
pub(super) fn generation_profile(
    type_id: &str,
) -> Result<&'static GenerationProfile, CommandError> {
    PROFILES
        .iter()
        .find(|profile| profile.type_id == type_id)
        .ok_or_else(|| CommandError::validation("当前卡组类型尚未注册 AI 生成规则"))
}
