use super::{generation_error, sources::ResolvedSource, GenerationCallError, GenerationSession};
use crate::models::GenerationInput;
use serde::Serialize;
use serde_json::Value;
use std::collections::HashSet;

#[derive(Clone, Debug, PartialEq)]
pub(super) struct PlanItem {
    pub id: String,
    pub keyword: String,
    pub resolved: Option<ResolvedSource>,
    pub emitted: bool,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PlanItemError {
    pub item_id: String,
    pub code: &'static str,
    pub message: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PendingItem {
    pub item_id: String,
    pub keyword: String,
    pub valid: bool,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PlanSummary {
    pub planned: usize,
    pub emitted: usize,
    pub pending: Vec<String>,
    pub pending_items: Vec<PendingItem>,
    pub errors: Vec<PlanItemError>,
    pub changed: bool,
}

impl GenerationSession {
    /// 每条独立校验并按稳定 ID 更新；遗漏不是删除，失败占位必须修复后才能结束。
    pub fn submit_plan(
        &mut self,
        input: &GenerationInput,
        arguments: Value,
    ) -> Result<PlanSummary, GenerationCallError> {
        let items = arguments
            .get("items")
            .and_then(Value::as_array)
            .ok_or_else(|| generation_error("INVALID_SCHEMA", "清单必须包含 items 数组"))?;

        let mut errors = Vec::new();
        let removals = arguments
            .get("removeItemIds")
            .map(|v| serde_json::from_value::<Vec<String>>(v.clone()))
            .transpose()
            .map_err(|_| generation_error("INVALID_SCHEMA", "removeItemIds 必须是 ID 数组"))?
            .unwrap_or_default();
        if items.is_empty() && removals.is_empty() {
            return Err(generation_error(
                "EMPTY_PLAN",
                "请提交考点或显式删除未落地项",
            ));
        }
        for id in removals {
            if let Some(index) = self.plan.iter().position(|p| p.id == id) {
                if self.plan[index].emitted {
                    errors.push(PlanItemError {
                        item_id: id,
                        code: "EMITTED_ITEM_IMMUTABLE",
                        message: "已落地条目不可删除".into(),
                    });
                } else {
                    self.plan.remove(index);
                }
            }
        }
        let mut seen = HashSet::new();
        for item in items {
            let supplied_id = item
                .get("itemId")
                .and_then(Value::as_str)
                .filter(|id| !id.trim().is_empty() && id.len() <= 128);
            // 老调用用考点名确定身份；无法识别的坏条目也返回可修复的稳定占位 ID。
            let keyword = item
                .get("keyword")
                .and_then(Value::as_str)
                .unwrap_or("")
                .trim()
                .to_string();
            let id = supplied_id.map(str::to_string).unwrap_or_else(|| {
                if !keyword.is_empty()
                    && item.get("itemId").is_none()
                    && item.get("source").is_some()
                {
                    format!("legacy:{keyword}")
                } else {
                    format!("invalid-item-{}", uuid::Uuid::now_v7())
                }
            });
            let existing = self.plan.iter().position(|p| p.id == id);
            let resolved = self.resolve_item(input, item);
            let error = if !seen.insert(id.clone()) {
                Some(generation_error(
                    "DUPLICATE_ITEM_ID",
                    "同一批次 itemId 不得重复",
                ))
            } else if supplied_id.is_none() && !id.starts_with("legacy:") {
                Some(generation_error(
                    "INVALID_ITEM_ID",
                    "请使用返回的 itemId 修复条目",
                ))
            } else if keyword.is_empty() {
                Some(generation_error("INVALID_PLAN_ITEM", "keyword 不能为空"))
            } else if self.plan.iter().any(|p| {
                p.id != id
                    && p.keyword.to_lowercase() == keyword.to_lowercase()
                    && p.resolved.is_some()
            }) {
                Some(generation_error(
                    "DUPLICATE_PLAN_ITEM",
                    "keyword 与已有条目重复",
                ))
            } else {
                resolved
                    .as_ref()
                    .err()
                    .map(|e| generation_error(e.code, e.message.clone()))
            };
            let next = PlanItem {
                id: id.clone(),
                keyword: keyword.clone(),
                resolved: resolved.ok(),
                emitted: false,
            };
            let error = if let Some(old) = existing.map(|i| &self.plan[i]).filter(|p| p.emitted) {
                if error.is_none() && old.keyword == next.keyword && old.resolved == next.resolved {
                    None
                } else {
                    Some(generation_error(
                        "EMITTED_ITEM_IMMUTABLE",
                        "已落地条目不可修改",
                    ))
                }
            } else {
                error
            };
            if let Some(error) = error {
                errors.push(PlanItemError {
                    item_id: id.clone(),
                    code: error.code,
                    message: error.message,
                });
                if existing.is_none() {
                    self.plan.push(PlanItem {
                        id,
                        keyword,
                        resolved: None,
                        emitted: false,
                    });
                }
                continue;
            }
            if let Some(index) = existing {
                if !self.plan[index].emitted && self.plan[index] != next {
                    self.plan[index] = next;
                }
            } else {
                self.plan.push(next);
            }
        }
        // 只以有效计划数量的历史新高判定进展，删除重加与反复修改不能无限续命。
        let valid_count = self.plan.iter().filter(|p| p.resolved.is_some()).count();
        let changed = valid_count > self.plan_high_water;
        self.plan_high_water = self.plan_high_water.max(valid_count);
        Ok(PlanSummary {
            planned: self.plan.len(),
            emitted: self.plan.iter().filter(|p| p.emitted).count(),
            pending: self.pending_keywords(),
            pending_items: self
                .plan
                .iter()
                .filter(|p| !p.emitted)
                .map(|p| PendingItem {
                    item_id: p.id.clone(),
                    keyword: p.keyword.clone(),
                    valid: p.resolved.is_some(),
                })
                .collect(),
            errors,
            changed,
        })
    }

    /// 服务层只需调用本方法，不能自行按重复摘录猜测计划条目。
    pub fn prepare_emit(
        &self,
        mut arguments: Value,
    ) -> Result<(usize, Value), GenerationCallError> {
        if !self.has_plan() {
            return Err(generation_error("PLAN_REQUIRED", "请先提交考点清单"));
        }
        let index = if let Some(id) = arguments.get("itemId") {
            let id = id
                .as_str()
                .ok_or_else(|| generation_error("INVALID_ITEM_ID", "itemId 必须是字符串"))?;
            self.plan.iter().position(|p| p.id == id)
        } else {
            arguments
                .get("source")
                .and_then(Value::as_str)
                .and_then(|source| self.plan_slot(source.trim()))
        }
        .ok_or_else(|| generation_error("NOT_PLANNED", "卡片必须引用清单中的 itemId"))?;
        let item = &self.plan[index];
        if item.emitted {
            return Err(generation_error(
                "ALREADY_EMITTED",
                "该条目已经落地，不要重复提交",
            ));
        }
        let resolved = item
            .resolved
            .as_ref()
            .ok_or_else(|| generation_error("INVALID_PLAN_ITEM", "请先修复该 itemId 的来源错误"))?;
        arguments["itemId"] = Value::String(item.id.clone());
        arguments["source"] = Value::String(resolved.excerpt.clone());
        Ok((index, arguments))
    }

    /// 占位条目也是计划的一部分，不能用空重试抹去错误。
    pub fn has_plan(&self) -> bool {
        !self.plan.is_empty()
    }

    /// 兼容旧摘录调用；新调用必须按 ID 对齐。
    pub fn plan_slot(&self, source: &str) -> Option<usize> {
        self.plan
            .iter()
            .position(|p| !p.emitted && p.resolved.as_ref().is_some_and(|r| r.excerpt == source))
    }

    /// 标记是幂等的；坏占位永远不能因下标调用变成已完成。
    pub fn mark_plan_emitted(&mut self, index: usize) {
        if let Some(item) = self.plan.get_mut(index).filter(|p| p.resolved.is_some()) {
            item.emitted = true;
        }
    }

    /// 结束闸门包含坏占位；新反馈另带结构化 itemId，避免修错条目。
    pub fn pending_keywords(&self) -> Vec<String> {
        self.plan
            .iter()
            .filter(|p| !p.emitted)
            .map(|p| {
                if p.keyword.is_empty() {
                    p.id.clone()
                } else {
                    p.keyword.clone()
                }
            })
            .collect()
    }
}
