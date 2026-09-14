import { computed, getCurrentScope, onScopeDispose, ref } from "vue";
import { planNoteMoves, type NoteMove, type NoteMoveItem } from "../../domain/notePaths";
import { rowKey, type TreeRow } from "./treeModel";

/** 升级为拖拽所需的指针位移，保证点击与双击不误触发拖动。 */
const DRAG_THRESHOLD = 4;
/** 悬停折叠文件夹后自动展开的等待时间。 */
const EXPAND_DELAY_MS = 600;

/** 拖拽会话：items 是整组待移动条目，folder/key 是当前有效落点。 */
export interface TreeDragSession {
  items: NoteMoveItem[];
  /** 有效落点目录（null 表示当前位置不能放下）。 */
  folder: string | null;
  /** 落点高亮的行键（null 表示根级空白）。 */
  key: string | null;
  /** 本次将移动的条目数，已扣除被父文件夹覆盖的后代。 */
  count: number;
}

/**
 * 文件树拖拽：按下阈值后进入拖拽，指针悬停决定落点，松手交出移动计划。
 *
 * 用指针事件而非 HTML5 拖放：桌面端 WebView2 在 dragDropEnabled 下会拦截
 * 拖放事件，指针事件在应用窗口与浏览器演示环境行为一致。
 * 只负责拖拽会话与落点判定，路径规则来自领域层，移动执行由调用方负责。
 */
export function useTreeDrag(options: {
  /** 拖拽开始时由调用方决定整组条目（并在此时同步选择集）。 */
  resolveItems: (row: TreeRow) => NoteMoveItem[];
  /** 悬停折叠文件夹时展开，便于拖进尚未展开的目录。 */
  expand: (path: string) => void;
  /** 判断文件夹是否已经展开。 */
  isExpanded: (path: string) => boolean;
  /** 松手落下：把移动计划交还调用方执行。 */
  onDrop: (moves: NoteMove[]) => void;
  /** 树容器：指针离开它就等于离开树，不作落点。 */
  container: () => HTMLElement | null;
}) {
  const drag = ref<TreeDragSession | null>(null);
  /** 幽灵标签位置单独存放：指针每次移动只重绘幽灵，不重算全树行样式。 */
  const ghost = ref<{ x: number; y: number } | null>(null);
  /** 已按下但未超过阈值的候选行。 */
  let press: { row: TreeRow; x: number; y: number } | null = null;
  /** 拖拽结束紧跟的 click 必须吞掉，否则会顺手打开笔记或折叠目录。 */
  let suppressClick = false;
  let expandTimer: ReturnType<typeof setTimeout> | null = null;
  /** 正在计时的展开目标，目标不变时不重置计时。 */
  let pendingExpand: string | null = null;

  /** 幽灵标签：单项显示名称，多项显示数量。 */
  const label = computed(() => {
    const session = drag.value;
    if (!session) return "";
    return session.items.length > 1 ? `${session.items.length} 项` : session.items[0]?.path.split("/").pop() ?? "";
  });

  /** 行是否属于本次拖拽（源行淡化显示）。 */
  function isDragging(row: TreeRow): boolean {
    const session = drag.value;
    return session ? session.items.some((item) => item.path === rowKey(row)) : false;
  }

  /** 行是否是当前落点（目标行高亮）。 */
  function isDropTarget(row: TreeRow): boolean {
    const key = drag.value?.key;
    return key != null && key === rowKey(row);
  }

  /** 清理悬停展开定时器。 */
  function clearExpandTimer(): void {
    if (expandTimer !== null) {
      clearTimeout(expandTimer);
      expandTimer = null;
    }
    pendingExpand = null;
  }

  /** 悬停在折叠文件夹上时延时展开；离开或目标变化立即取消。 */
  function scheduleExpand(folder: string | null): void {
    if (folder === pendingExpand) return;
    clearExpandTimer();
    if (!folder || options.isExpanded(folder)) return;
    pendingExpand = folder;
    expandTimer = setTimeout(() => {
      expandTimer = null;
      pendingExpand = null;
      options.expand(folder);
    }, EXPAND_DELAY_MS);
  }

  /** 更新落点：null 表示指针已离开可放置区域；目标未变化时不触发重绘。 */
  function setTarget(target: { folder: string; key: string | null } | null): void {
    const session = drag.value;
    if (!session) return;
    const count = target ? planNoteMoves(session.items, target.folder).length : 0;
    const folder = target && count > 0 ? target.folder : null;
    const key = target && count > 0 ? target.key : null;
    const total = folder === null ? 0 : count;
    scheduleExpand(folder);
    if (session.folder === folder && session.key === key && session.count === total) return;
    drag.value = { ...session, folder, key, count: total };
  }

  /** 解绑窗口级监听。 */
  function release(): void {
    window.removeEventListener("pointermove", onPointerMove);
    window.removeEventListener("pointerup", onDropPointer);
    window.removeEventListener("pointercancel", cancel);
    window.removeEventListener("keydown", onKeyDown);
    window.removeEventListener("blur", cancel);
  }

  /** 取消拖拽：Escape、窗口失焦与组件卸载都走这里，不执行任何移动。 */
  function cancel(): void {
    release();
    clearExpandTimer();
    drag.value = null;
    ghost.value = null;
    press = null;
  }

  /** 松手落下：有有效落点才产生移动计划，随后无论结果如何都结束会话。 */
  function onDropPointer(): void {
    const session = drag.value;
    const moves = session && session.folder !== null ? planNoteMoves(session.items, session.folder) : [];
    // 落进尚未展开的目录时展开它，移动结果要立刻看得见。
    if (moves.length > 0 && session?.folder) {
      options.expand(session.folder);
    }
    release();
    clearExpandTimer();
    drag.value = null;
    ghost.value = null;
    press = null;
    if (moves.length > 0) options.onDrop(moves);
  }

  /** Escape 取消拖拽，避免误放手。 */
  function onKeyDown(event: KeyboardEvent): void {
    if (event.key !== "Escape" || !drag.value) return;
    event.preventDefault();
    cancel();
  }

  /** 指针移动：超过阈值才升级为拖拽，之后持续更新幽灵位置与落点。 */
  function onPointerMove(event: PointerEvent): void {
    const start = press;
    if (!start) return;
    let session = drag.value;
    if (!session) {
      if (Math.abs(event.clientX - start.x) < DRAG_THRESHOLD && Math.abs(event.clientY - start.y) < DRAG_THRESHOLD) return;
      session = { items: options.resolveItems(start.row), folder: null, key: null, count: 0 };
      drag.value = session;
      // 指针拖过行内文字时浏览器仍会顺手选中，拖拽期间清掉。
      window.getSelection()?.removeAllRanges();
      suppressClick = true;
    }
    ghost.value = { x: event.clientX, y: event.clientY };
    // 指针事件的目标就是指针下的元素；落在树上就按行判定，树外一律不放置。
    const element = event.target instanceof Element ? event.target : null;
    const container = options.container();
    if (!element || !container || !container.contains(element)) {
      setTarget(null);
      return;
    }
    const row = element.closest<HTMLElement>("[data-drop-folder]");
    // 没有命中行说明指针在树内空白处，落点是根目录。
    setTarget({ folder: row?.dataset.dropFolder ?? "", key: row?.dataset.treeKey ?? null });
  }

  /** 在行上按下左键：只记录起点与候选行，不阻止点击与右键菜单。 */
  function onPointerDown(row: TreeRow, event: PointerEvent): void {
    if (event.button !== 0 || drag.value) return;
    suppressClick = false;
    press = { row, x: event.clientX, y: event.clientY };
    window.addEventListener("pointermove", onPointerMove);
    window.addEventListener("pointerup", onDropPointer);
    window.addEventListener("pointercancel", cancel);
    window.addEventListener("keydown", onKeyDown);
    window.addEventListener("blur", cancel);
  }

  /** 拖拽结束后的首次 click 只用于收尾，不能再触发行点击。 */
  function guardClick(event: MouseEvent): void {
    if (!suppressClick) return;
    suppressClick = false;
    event.stopPropagation();
    event.preventDefault();
  }

  if (getCurrentScope()) {
    onScopeDispose(cancel);
  }

  return { drag, ghost, label, isDragging, isDropTarget, onPointerDown, guardClick, cancel };
}
