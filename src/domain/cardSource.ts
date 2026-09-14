import type { CardSource } from "./types";

/** 来源坐标按编辑器 UTF-16 文档计算，附带上下文防止误命中重复文本。 */
export function captureCardSource(document: string, from: number, to: number): CardSource {
  let prefixStart = Math.max(0, from - 48);
  let suffixEnd = Math.min(document.length, to + 48);
  // 上下文不切开 UTF-16 代理对，保证 JSON 可被 Rust 解码并与其来源策略一致。
  if (prefixStart > 0 && isSurrogateBoundary(document, prefixStart)) prefixStart += 1;
  if (suffixEnd < document.length && isSurrogateBoundary(document, suffixEnd)) suffixEnd -= 1;
  return { from, to, excerpt: document.slice(from, to),
    prefix: document.slice(prefixStart, from), suffix: document.slice(to, suffixEnd) };
}

/** 仅当偏移落在高低代理项之间时才向内收缩上下文边界。 */
function isSurrogateBoundary(document: string, offset: number): boolean {
  return document.charCodeAt(offset - 1) >= 0xd800 && document.charCodeAt(offset - 1) <= 0xdbff
    && document.charCodeAt(offset) >= 0xdc00 && document.charCodeAt(offset) <= 0xdfff;
}

/** 仅接受原位置仍匹配的来源，不搜索第一处相同文本来猜测位置。 */
export function isCardSourceCurrent(document: string, source: CardSource): boolean {
  return source.from >= 0 && source.to > source.from && source.to <= document.length
    && document.slice(source.from, source.to) === source.excerpt
    && document.slice(Math.max(0, source.from - source.prefix.length), source.from) === source.prefix
    && document.slice(source.to, source.to + source.suffix.length) === source.suffix;
}
