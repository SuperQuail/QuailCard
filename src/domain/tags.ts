import type { NoteSummary } from "./types";

/** 标签树节点：fullPath 是 a/b/c 形式的完整路径，count 是包含子标签后的去重笔记数。 */
export interface TagNode {
  /** 最后一段名称，用于展示。 */
  name: string;
  /** 从根到本节点的完整路径，用于筛选与折叠状态。 */
  fullPath: string;
  /** 带本标签及其全部子标签的笔记数（同一篇笔记只算一次）。 */
  count: number;
  /** 子标签，已按名称排序。 */
  children: TagNode[];
}

/** 标签归一化：去掉开头的 #、折叠多余斜杠、丢弃空段；标签正文保留大小写。 */
export function normalizeTag(value: unknown): string {
  if (typeof value !== "string") return "";
  return value
    .trim()
    .replace(/^#+/, "")
    .split("/")
    .map((part) => part.trim())
    .filter(Boolean)
    .join("/");
}

/**
 * 解析笔记摘要里的 tagsJson。
 * 契约：读取必须容错——非法 JSON、非数组、非字符串项都按"无标签"处理，
 * 绝不因为一篇坏数据让整个标签面板崩掉。
 */
export function tagsOf(note: Pick<NoteSummary, "tagsJson">): string[] {
  let parsed: unknown;
  try {
    parsed = JSON.parse(note.tagsJson);
  } catch {
    return [];
  }
  if (!Array.isArray(parsed)) return [];
  const tags: string[] = [];
  for (const item of parsed) {
    const tag = normalizeTag(item);
    if (tag && !tags.includes(tag)) tags.push(tag);
  }
  return tags;
}

/** 建树时的可变节点：notes 集合承担"父计数 = 子树去重笔记数"的职责。 */
interface MutableTagNode {
  name: string;
  fullPath: string;
  notes: Set<string>;
  children: Map<string, MutableTagNode>;
}

/** 子标签按名称排序，保证同一份数据每次渲染顺序一致。 */
function sortedChildren(nodes: Map<string, MutableTagNode>): TagNode[] {
  return [...nodes.values()]
    .sort((left, right) => left.name.localeCompare(right.name, "zh-CN"))
    .map((node) => ({
      name: node.name,
      fullPath: node.fullPath,
      count: node.notes.size,
      children: sortedChildren(node.children),
    }));
}

/**
 * 把全部笔记的标签展开成树。
 * a/b/c 会在 a、a/b、a/b/c 三级各记一次这篇笔记，所以父节点的 count
 * 天然等于"自身加所有子标签去重后的笔记数"，不需要再做合并。
 */
export function buildTagTree(notes: NoteSummary[]): TagNode[] {
  const roots = new Map<string, MutableTagNode>();
  for (const note of notes) {
    for (const tag of tagsOf(note)) {
      let level = roots;
      let fullPath = "";
      for (const part of tag.split("/")) {
        fullPath = fullPath ? `${fullPath}/${part}` : part;
        let node = level.get(part);
        if (!node) {
          node = { name: part, fullPath, notes: new Set(), children: new Map() };
          level.set(part, node);
        }
        node.notes.add(note.path);
        level = node.children;
      }
    }
  }
  return sortedChildren(roots);
}

/**
 * 按标签筛选笔记：选中 a 时 a/b、a/b/c 也会命中，符合"父标签包含子标签"的直觉。
 * 空标签返回全部笔记（调用方用它表达"未筛选"），标签比较沿用归一化后的路径。
 */
export function filterNotesByTag(notes: NoteSummary[], tag: string): NoteSummary[] {
  const wanted = normalizeTag(tag);
  if (!wanted) return [...notes];
  const prefix = `${wanted}/`;
  return notes.filter((note) =>
    tagsOf(note).some((item) => item === wanted || item.startsWith(prefix)));
}
