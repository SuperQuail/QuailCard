import type { NoteSummary } from "./types";

/** 双链目标的最小信息：解析只需要路径、标题与别名。 */
export type LinkTarget = Pick<NoteSummary, "path" | "title" | "aliases">;

/**
 * 归一化待比较的字符串：统一分隔符、去掉 .md 后缀与 ./ 前缀、忽略大小写。
 * 双链在 Obsidian 里天然不区分大小写，统一转小写后比较才能与后端一致。
 */
function normalize(value: string): string {
  return value
    .trim()
    .replace(/\\/g, "/")
    .replace(/^\.\//, "")
    .replace(/\.md$/i, "")
    .replace(/\/+$/, "")
    .toLowerCase();
}

/** 取归一化后的所在目录，根目录为空串。 */
function directoryOf(path: string): string {
  const normalized = normalize(path);
  const slash = normalized.lastIndexOf("/");
  return slash < 0 ? "" : normalized.slice(0, slash);
}

/**
 * 同名候选的优先级：同目录 > 路径更短 > 路径字典序。
 * 这套顺序让"就近引用"优先，同时保证结果稳定可预测。
 */
function pickCandidate(candidates: LinkTarget[], fromDirectory: string): string | null {
  if (candidates.length === 0) {
    return null;
  }
  const ranked = [...candidates].sort((left, right) => {
    const leftSame = directoryOf(left.path) === fromDirectory ? 0 : 1;
    const rightSame = directoryOf(right.path) === fromDirectory ? 0 : 1;
    if (leftSame !== rightSame) return leftSame - rightSame;
    if (left.path.length !== right.path.length) return left.path.length - right.path.length;
    return left.path.localeCompare(right.path);
  });
  return ranked[0]?.path ?? null;
}

/**
 * 把 [[目标]] 解析成 Vault 内的笔记路径，解析不到返回 null。
 *
 * 解析顺序（与后端 storage/links.rs 必须完全一致，由 link_resolution.json 夹具锁定）：
 * 1. Vault 相对路径精确命中；
 * 2. 相对当前笔记所在目录命中；
 * 3. 标题命中（同名时按同目录 > 最短路径 > 字典序）；
 * 4. 别名命中（排序规则同上）。
 */
export function resolveLink(target: string, fromPath: string, notes: LinkTarget[]): string | null {
  const wanted = normalize(target);
  if (!wanted) {
    return null;
  }
  const direct = notes.find((note) => normalize(note.path) === wanted);
  if (direct) {
    return direct.path;
  }
  const fromDirectory = directoryOf(fromPath);
  const relative = fromDirectory ? `${fromDirectory}/${wanted}` : wanted;
  const nearby = notes.find((note) => normalize(note.path) === relative);
  if (nearby) {
    return nearby.path;
  }
  const byTitle = notes.filter((note) => normalize(note.title) === wanted);
  const titleHit = pickCandidate(byTitle, fromDirectory);
  if (titleHit) {
    return titleHit;
  }
  const byAlias = notes.filter((note) => (note.aliases ?? []).some((alias) => normalize(alias) === wanted));
  return pickCandidate(byAlias, fromDirectory);
}
