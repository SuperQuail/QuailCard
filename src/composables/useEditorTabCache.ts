import type { EditorState } from "@codemirror/state";
import type { EditorView } from "@codemirror/view";
import type { EditorReadingSnapshot } from "./useEditorReadingMode";

interface EditorTabSnapshot {
  state?: EditorState;
  reading: EditorReadingSnapshot;
  scroll?: { top: number; left: number; effect: ReturnType<EditorView["scrollSnapshot"]> };
}

/** 只缓存非活跃笔记的不可变状态；上限约束历史内存，不创建后台编辑器。 */
export function useEditorTabCache(limit = 20) {
  const entries = new Map<string, EditorTabSnapshot>();
  const capacity = Math.max(0, Math.floor(limit));

  /** 离开时记录状态与滚动锚点；重新放入尾部使最久未访问笔记优先淘汰。 */
  function save(path: string, view: EditorView, reading: EditorReadingSnapshot): void {
    entries.delete(path);
    entries.set(path, {
      state: view.state, reading,
      scroll: { top: view.scrollDOM.scrollTop, left: view.scrollDOM.scrollLeft, effect: view.scrollSnapshot() },
    });
    while (entries.size > capacity) entries.delete(entries.keys().next().value!);
  }

  /** 活跃状态移出缓存；正文不符时以外部内容重建，不能把旧草稿或撤销栈带回。 */
  function take(path: string, content: string): EditorTabSnapshot | undefined {
    const saved = entries.get(path);
    entries.delete(path);
    if (!saved || saved.state?.doc.toString() === content) return saved;
    return { reading: { reading: saved.reading.reading, editSelection: null } };
  }

  /** 标签列表是生命周期边界，关闭、删除、改名的非活跃记录不应被再次复用。 */
  function retain(paths: readonly string[]): void {
    const open = new Set(paths);
    for (const path of entries.keys()) if (!open.has(path)) entries.delete(path);
  }

  /** 直接位置供首帧使用，CodeMirror 滚动快照在布局测量后恢复准确锚点。 */
  function restoreScroll(view: EditorView, saved?: EditorTabSnapshot): void {
    if (!saved?.scroll) return;
    view.scrollDOM.scrollTop = saved.scroll.top;
    view.scrollDOM.scrollLeft = saved.scroll.left;
  }

  /** 卸载或换 Vault 时主动释放所有历史，缓存绝不跨会话持久化。 */
  function clear(): void { entries.clear(); }

  return { save, take, retain, restoreScroll, clear };
}
