/** 按路径边界迁移笔记或目录，避免 old 错误匹配 older。 */
export function remapNotePath(path: string, oldPath: string, newPath: string): string {
  return path === oldPath || path.startsWith(`${oldPath}/`) ? `${newPath}${path.slice(oldPath.length)}` : path;
}

export interface NotePathChange {
  oldPath: string;
  newPath: string;
}

/** 拖拽移动条目：path 是笔记或文件夹的完整相对路径。 */
export interface NoteMoveItem {
  kind: "folder" | "note";
  path: string;
}

/** 一次移动：from/to 都是完整相对路径，to 已含目标目录。 */
export interface NoteMove {
  kind: "folder" | "note";
  from: string;
  to: string;
}

/** 所在目录路径（根级条目返回空字符串）。 */
export function parentPath(path: string): string {
  const index = path.lastIndexOf("/");
  return index >= 0 ? path.slice(0, index) : "";
}

/** 取最后一段路径作为名称。 */
function pathName(path: string): string {
  const index = path.lastIndexOf("/");
  return index >= 0 ? path.slice(index + 1) : path;
}

/** 条目能否移入目标目录：原地不动与文件夹移进自身子树都不允许。 */
export function canMoveInto(item: NoteMoveItem, folder: string): boolean {
  if (item.kind === "folder" && (item.path === folder || folder.startsWith(`${item.path}/`))) {
    return false;
  }
  return parentPath(item.path) !== folder;
}

/**
 * 计算一次拖拽的移动计划。
 *
 * 跳过无效落点，并丢弃已被选中父文件夹覆盖的后代：父文件夹先移动后，
 * 后代的旧路径已不存在。重名冲突交给后端判定，前端不猜测结果。
 */
export function planNoteMoves(items: NoteMoveItem[], folder: string): NoteMove[] {
  const folders = items.filter((item) => item.kind === "folder").map((item) => item.path);
  return items
    .filter((item) => !folders.some((path) => item.path.startsWith(`${path}/`)))
    .filter((item) => canMoveInto(item, folder))
    .map((item) => ({
      kind: item.kind,
      from: item.path,
      to: folder ? `${folder}/${pathName(item.path)}` : pathName(item.path),
    }));
}
