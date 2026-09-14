import type { ProviderModel } from "./types";

/**
 * 模型目录的纯函数：token 数量解析、格式化与整表规范化。
 *
 * 契约：这里只做无副作用的输入整理，不发请求、不读全局状态；
 * 后端 storage::validate_provider 持有同样的边界，两侧必须保持一致。
 */

/** 上下文窗口下限：低于它不可能是真实模型，视为填写错误。 */
export const MIN_CONTEXT_WINDOW = 1024;

/** 单次输出上限上限值，防止把请求参数写成天文数字。 */
export const MAX_OUTPUT_TOKENS = 200_000;

/** 未配置最大输出 token 时后端使用的默认值（与 Rust 侧一致）。 */
export const DEFAULT_MAX_OUTPUT_TOKENS = 32768;

/**
 * 解析人写的 token 数量：支持 "131072"、"32K"、"1M"（不区分大小写）。
 * 空串返回 null（表示未设置）；非法或非正数同样返回 null。
 */
export function parseTokenCount(text: string): number | null {
  const trimmed = text.trim();
  if (!trimmed) {
    return null;
  }
  const matched = /^(\d+(?:\.\d+)?)\s*([kKmM])?$/.exec(trimmed);
  if (!matched) {
    return null;
  }
  const base = Number(matched[1]);
  if (!Number.isFinite(base) || base <= 0) {
    return null;
  }
  const suffix = (matched[2] ?? "").toLowerCase();
  const scale = suffix === "m" ? 1024 * 1024 : suffix === "k" ? 1024 : 1;
  return Math.round(base * scale);
}

/** 反向格式化：能整除时显示 1M / 32K，否则原样显示数字。 */
export function formatTokenCount(value: number | null): string {
  if (value === null || !Number.isFinite(value) || value <= 0) {
    return "";
  }
  if (value % (1024 * 1024) === 0) {
    return `${value / (1024 * 1024)}M`;
  }
  if (value % 1024 === 0) {
    return `${value / 1024}K`;
  }
  return String(value);
}

/**
 * 返回该模型的填写问题；null 表示可以保存。
 * 显示名称留空时按模型 id 处理，不视为错误。
 */
export function modelIssue(model: ProviderModel): string | null {
  if (!model.id.trim()) {
    return "请填写模型 ID";
  }
  if (model.contextWindow !== null && model.contextWindow < MIN_CONTEXT_WINDOW) {
    return `上下文窗口不能小于 ${MIN_CONTEXT_WINDOW}`;
  }
  if (
    model.maxOutputTokens !== null
    && (model.maxOutputTokens < 1 || model.maxOutputTokens > MAX_OUTPUT_TOKENS)
  ) {
    return `最大输出 token 需在 1–${MAX_OUTPUT_TOKENS} 之间`;
  }
  return null;
}

/**
 * 整表规范化：去空白、丢掉空 id、按 id 去重、补默认显示名称。
 * 后端的 required 校验仍以规范化后的结果为准，避免界面提交“看起来有值”的空条目。
 */
export function normalizeModels(models: ProviderModel[]): ProviderModel[] {
  const seen = new Set<string>();
  const normalized: ProviderModel[] = [];
  for (const model of models) {
    const id = model.id.trim();
    if (!id || seen.has(id)) {
      continue;
    }
    seen.add(id);
    normalized.push({
      id,
      name: model.name.trim() || id,
      contextWindow: model.contextWindow,
      maxOutputTokens: model.maxOutputTokens,
    });
  }
  return normalized;
}
