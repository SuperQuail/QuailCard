import { ref } from "vue";

/** 右键菜单目标：kind 决定菜单项，key 是目标路径。 */
export interface ContextTarget {
  kind: "folder" | "note" | "blank";
  key: string;
  x: number;
  y: number;
}

/** 菜单尺寸（视口钳制用）：宽度与最长变体的高度。 */
const MENU_WIDTH = 160;
const FOLDER_MENU_HEIGHT = 130;
const SIMPLE_MENU_HEIGHT = 96;

/**
 * 文件树右键菜单状态：打开时按视口尺寸钳制坐标，避免菜单溢出屏幕。
 * 只管理坐标与目标；菜单项动作由 FileTree 接线。
 */
export function useTreeContextMenu() {
  const context = ref<ContextTarget | null>(null);

  /** 计算钳制后的菜单坐标。 */
  function clamp(x: number, y: number, menuHeight: number): { x: number; y: number } {
    return {
      x: Math.min(x, window.innerWidth - MENU_WIDTH),
      y: Math.min(y, window.innerHeight - menuHeight),
    };
  }

  /** 打开文件夹右键菜单。 */
  function openFolderMenu(path: string, event: MouseEvent): void {
    event.preventDefault();
    const { x, y } = clamp(event.clientX, event.clientY, FOLDER_MENU_HEIGHT);
    context.value = { kind: "folder", key: path, x, y };
  }

  /** 打开笔记右键菜单。 */
  function openNoteMenu(path: string, event: MouseEvent): void {
    event.preventDefault();
    const { x, y } = clamp(event.clientX, event.clientY, SIMPLE_MENU_HEIGHT);
    context.value = { kind: "note", key: path, x, y };
  }

  /** 打开树区空白处的右键菜单。 */
  function openBlankMenu(event: MouseEvent): void {
    event.preventDefault();
    const { x, y } = clamp(event.clientX, event.clientY, SIMPLE_MENU_HEIGHT);
    context.value = { kind: "blank", key: "", x, y };
  }

  /** 关闭菜单。 */
  function closeMenu(): void {
    context.value = null;
  }

  return { context, openFolderMenu, openNoteMenu, openBlankMenu, closeMenu };
}
