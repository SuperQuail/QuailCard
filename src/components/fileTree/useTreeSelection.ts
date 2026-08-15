import { ref, type ComputedRef } from "vue";
import { rowKey, type TreeRow } from "./treeModel";

/** 待批量删除条目：kind 区分文件夹与笔记。 */
export interface DeleteItem {
  kind: "folder" | "note";
  path: string;
}

/**
 * 文件树多选状态：Shift 点击选范围、Ctrl/Cmd 点击切换、Delete 批量删除。
 * 只维护选择集合，不执行删除；删除由调用方通过回调落地。
 */
export function useTreeSelection(rows: ComputedRef<TreeRow[]>) {
  const selection = ref<Set<string>>(new Set());
  const selectionAnchor = ref<string | null>(null);

  /** 行点击选择逻辑：Shift 范围、Ctrl 切换、普通单选。 */
  function selectRow(row: TreeRow, event: MouseEvent): void {
    const key = rowKey(row);
    const keys = rows.value.map(rowKey);
    if (event.shiftKey && selectionAnchor.value) {
      const start = keys.indexOf(selectionAnchor.value);
      const end = keys.indexOf(key);
      if (start >= 0 && end >= 0) {
        const [low, high] = start <= end ? [start, end] : [end, start];
        selection.value = new Set(keys.slice(low, high + 1));
        return;
      }
    }
    if (event.ctrlKey || event.metaKey) {
      const next = new Set(selection.value);
      if (next.has(key)) {
        next.delete(key);
      } else {
        next.add(key);
      }
      selection.value = next;
      selectionAnchor.value = key;
      return;
    }
    selection.value = new Set([key]);
    selectionAnchor.value = key;
  }

  /** 判断行是否被选中。 */
  function isSelected(row: TreeRow): boolean {
    return selection.value.has(rowKey(row));
  }

  /** 将当前选择转换为删除条目（文件夹/笔记由行类型决定）。 */
  function buildDeleteItems(): DeleteItem[] {
    return rows.value
      .filter((row) => selection.value.has(rowKey(row)))
      .map((row) => ({
        kind: (row.kind === "folder" ? "folder" : "note") as "folder" | "note",
        path: rowKey(row),
      }));
  }

  /** 清空选择与锚点。 */
  function clearSelection(): void {
    selection.value = new Set();
    selectionAnchor.value = null;
  }

  return { selection, selectionAnchor, selectRow, isSelected, buildDeleteItems, clearSelection };
}
