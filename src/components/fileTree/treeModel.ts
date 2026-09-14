import type { NoteSummary } from "../../domain/types";

/** 树节点：path 是文件夹完整相对路径，根笔记挂在 path 为 "" 的虚拟节点。 */
export interface TreeFolder {
  name: string;
  path: string;
  children: TreeFolder[];
  notes: NoteSummary[];
}

/** 扁平渲染行：模板 v-for 的输入。 */
export type TreeRow =
  | { kind: "folder"; node: TreeFolder; depth: number }
  | { kind: "note"; note: NoteSummary; depth: number };

/** 从笔记路径与已建空文件夹推导完整文件夹列表（排序稳定）。 */
export function deriveFolderNames(notes: NoteSummary[], extraFolders: string[]): string[] {
  const folders = new Set<string>(extraFolders);
  for (const note of notes) {
    const parts = note.path.split("/");
    for (let index = 1; index < parts.length; index += 1) {
      folders.add(parts.slice(0, index).join("/"));
    }
  }
  return [...folders].sort();
}

/** 确保某路径的文件夹节点存在并返回它。 */
function ensureFolderNode(nodes: Map<string, TreeFolder>, root: TreeFolder[], path: string): TreeFolder {
  const existing = nodes.get(path);
  if (existing) {
    return existing;
  }
  const parts = path.split("/");
  let level = root;
  let currentPath = "";
  let node: TreeFolder | undefined;
  for (const part of parts) {
    const parentPath = currentPath;
    currentPath = parentPath ? `${parentPath}/${part}` : part;
    node = nodes.get(currentPath);
    if (!node) {
      node = { name: part, path: currentPath, children: [], notes: [] };
      nodes.set(currentPath, node);
      level.push(node);
    }
    level = node.children;
  }
  return node as TreeFolder;
}

/** 从文件夹名与笔记路径构建目录树（纯函数，可测试）。 */
export function buildTree(folderNames: string[], notes: NoteSummary[]): TreeFolder[] {
  const root: TreeFolder[] = [];
  const nodes = new Map<string, TreeFolder>();
  for (const path of folderNames) {
    ensureFolderNode(nodes, root, path);
  }
  for (const note of notes) {
    const parts = note.path.split("/");
    if (parts.length === 1) {
      root.push({ name: "", path: "", children: [], notes: [note] });
      continue;
    }
    const folder = parts.slice(0, -1).join("/");
    ensureFolderNode(nodes, root, folder).notes.push(note);
  }
  return root;
}

/** 文件夹是否有可展开的内容。 */
export function hasChildren(node: TreeFolder): boolean {
  return node.children.length > 0 || node.notes.length > 0;
}

/** 统计文件夹及其子文件夹中的笔记总数（折叠徽章用）。 */
export function countNotes(node: TreeFolder): number {
  let total = node.notes.length;
  for (const child of node.children) {
    total += countNotes(child);
  }
  return total;
}

/** 按展开状态把树展平为渲染行。 */
export function flattenRows(tree: TreeFolder[], expanded: Set<string>): TreeRow[] {
  const rows: TreeRow[] = [];
  const walk = (nodes: TreeFolder[], depth: number): void => {
    for (const node of nodes) {
      if (node.path === "") {
        for (const note of node.notes) {
          rows.push({ kind: "note", note, depth: 0 });
        }
        continue;
      }
      rows.push({ kind: "folder", node, depth });
      if (expanded.has(node.path)) {
        walk(node.children, depth + 1);
        for (const note of node.notes) {
          rows.push({ kind: "note", note, depth: depth + 1 });
        }
      }
    }
  };
  walk(tree, 0);
  return rows;
}

/** 在树中按路径查找文件夹节点。 */
export function findFolderNode(tree: TreeFolder[], path: string): TreeFolder | null {
  for (const node of tree) {
    if (node.path === path) {
      return node;
    }
    const found = findFolderNode(node.children, path);
    if (found) {
      return found;
    }
  }
  return null;
}

/** 行的唯一键（文件夹与笔记都以路径为键）。 */
export function rowKey(row: TreeRow): string {
  return row.kind === "folder" ? row.node.path : row.note.path;
}

/** 行缩进位置（像素）。 */
export function rowIndent(depth: number): string {
  return `${8 + depth * 16}px`;
}

/** 笔记所在的文件夹路径（根笔记返回 null）。 */
export function noteFolder(notes: NoteSummary[], notePath: string | null): string | null {
  const path = notes.find((note) => note.path === notePath)?.path ?? "";
  const parts = path.split("/");
  parts.pop();
  return parts.join("/") || null;
}
