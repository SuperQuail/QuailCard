import { nextTick, ref } from "vue";

/** 新建中的条目：parent 为 null 表示根级。 */
export interface CreatingState {
  kind: "note" | "folder";
  parent: string | null;
}

/** 重命名中的条目：value 与输入框双向绑定。 */
export interface RenamingState {
  kind: "folder" | "note";
  key: string;
  value: string;
}

/**
 * 文件树行内编辑状态：树内新建（根级或文件夹下）与重命名。
 * 提交动作通过回调交还调用方（emit 由 FileTree 负责），输入框聚焦用回调 ref 收集。
 */
export function useTreeEditing() {
  const creating = ref<CreatingState | null>(null);
  const createValue = ref("");
  const renaming = ref<RenamingState | null>(null);
  /** 当前渲染的新建输入框（v-for 内的 ref 会被收集成数组，改用回调 ref）。 */
  let createInputElement: HTMLInputElement | null = null;

  /** 回调 ref：记录实际渲染的新建输入框。 */
  function setCreateInput(element: unknown): void {
    if (element instanceof HTMLInputElement) {
      createInputElement = element;
    } else if (element === null) {
      createInputElement = null;
    }
  }

  /** 进入新建模式：展开父文件夹并聚焦输入框。 */
  function startCreating(kind: "note" | "folder", parent: string | null, onExpand?: (path: string) => void): void {
    creating.value = { kind, parent };
    createValue.value = "";
    if (parent && onExpand) {
      onExpand(parent);
    }
    void nextTick(() => createInputElement?.focus());
  }

  /** 提交新建：把结果交还回调，由调用方 emit。 */
  function commitCreate(onCreate: (kind: "note" | "folder", parent: string | null, value: string) => void): void {
    const state = creating.value;
    const value = createValue.value.trim();
    if (!state || !value) {
      return;
    }
    onCreate(state.kind, state.parent, value);
    creating.value = null;
  }

  /** 取消新建。 */
  function cancelCreate(): void {
    creating.value = null;
  }

  /** 进入重命名模式：预填当前名称。 */
  function startRenaming(kind: "folder" | "note", key: string, currentName: string): void {
    renaming.value = { kind, key, value: currentName };
  }

  /** 由旧路径和新名称计算完整新路径（笔记自动补 .md 后缀）。 */
  function renamePath(oldPath: string, newName: string, isFolder: boolean): string {
    const index = oldPath.lastIndexOf("/");
    const parent = index >= 0 ? oldPath.slice(0, index) : "";
    let name = newName.trim();
    if (!isFolder && !name.toLowerCase().endsWith(".md")) {
      name = `${name}.md`;
    }
    return parent ? `${parent}/${name}` : name;
  }

  /** 提交重命名：把新路径交还回调，由调用方 emit。 */
  function commitRename(onRename: (kind: "folder" | "note", oldPath: string, newPath: string) => void): void {
    const state = renaming.value;
    if (!state) {
      return;
    }
    const value = state.value.trim();
    if (value) {
      onRename(state.kind, state.key, renamePath(state.key, value, state.kind === "folder"));
    }
    renaming.value = null;
  }

  /** 取消重命名。 */
  function cancelRename(): void {
    renaming.value = null;
  }

  return {
    creating,
    createValue,
    renaming,
    setCreateInput,
    startCreating,
    commitCreate,
    cancelCreate,
    startRenaming,
    commitRename,
    cancelRename,
  };
}
