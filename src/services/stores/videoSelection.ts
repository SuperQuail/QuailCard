import type { VideoProbe } from "../../domain/video";
import { extractLink } from "../videoLink";

/** 显式分 P 参数仅作为选择提示；短链解析结果优先由后端补充。 */
export function inputPage(text: string): number | undefined {
  try {
    const value = new URL(extractLink(text)).searchParams.get("p");
    const page = Number(value);
    return Number.isSafeInteger(page) && page > 0 ? page : undefined;
  } catch { return undefined; }
}

/** 自动档优先 480P，再降档；没有低档时取最低可用档，绝不依赖接口顺序。 */
export function chooseQuality(probe: VideoProbe, preferred = "auto"): number {
  const available = probe.qualities.filter(item => item.available);
  const target = preferred === "auto" ? 32 : Number(preferred);
  const exact = available.find(item => item.qn === target);
  if (exact) return exact.qn;
  const descending = [...available].sort((a, b) => b.qn - a.qn);
  return descending.find(item => item.qn <= target)?.qn ?? descending[descending.length - 1]?.qn ?? 0;
}
