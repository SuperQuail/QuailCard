import { ref } from "vue";
import { Annotation, Compartment, EditorState, type EditorSelection, type Extension } from "@codemirror/state";
import { EditorView, highlightActiveLine, type ViewUpdate } from "@codemirror/view";
import { readingMode } from "../editor/readingMode";

/** 已确认的外部草稿同步可更新阅读内容，用户编辑事务则必须被拒绝。 */
export const externalNoteUpdate = Annotation.define<boolean>();

/** 仅保存阅读开关与编辑选区，不引用编辑器 DOM 或监听器。 */
export interface EditorReadingSnapshot {
  reading: boolean;
  editSelection: EditorSelection | null;
}

/** 阅读是视图状态；复用同一编辑器以保留草稿、选区和撤销栈。 */
export function useEditorReadingMode(getView: () => EditorView | null, isBusy: () => boolean) {
  const reading = ref(false);
  const compartment = new Compartment();
  let editSelection: EditorSelection | null = null;

  /** 文件忙碌与阅读分别控制状态，退出阅读不能解除文件操作锁。 */
  function configuration(): Extension {
    const locked = reading.value || isBusy();
    return [
      readingMode.of(reading.value),
      EditorState.readOnly.of(locked),
      EditorView.editable.of(!locked),
      EditorView.contentAttributes.of(reading.value
        ? { role: "document", "aria-label": "笔记阅读", tabindex: "0" }
        : { "aria-label": "笔记编辑" }),
      EditorView.editorAttributes.of({ "data-reading": String(reading.value) }),
      reading.value ? [] : highlightActiveLine(),
    ];
  }

  /** 原生输入以外的撤销或异步扩展也不能绕过阅读锁；外部同步显式放行。 */
  function extensions(): Extension {
    return [
      compartment.of(configuration()),
      EditorState.transactionFilter.of((transaction) => {
        if (transaction.docChanged && transaction.startState.facet(readingMode) && !transaction.annotation(externalNoteUpdate)) return [];
        return transaction;
      }),
      EditorView.theme({
        '&[data-reading="true"] .cm-content': { cursor: "text", caretColor: "transparent" },
        '&[data-reading="true"] .cm-cursorLayer': { display: "none" },
      }),
    ];
  }

  /** 只重配交互扩展，不重建文档或历史；返回编辑时恢复原选区。 */
  function toggle(): void {
    const view = getView();
    if (!view) return;
    if (!reading.value) editSelection = view.state.selection;
    reading.value = !reading.value;
    view.dispatch({
      effects: compartment.reconfigure(configuration()),
      selection: !reading.value && editSelection ? editSelection : undefined,
    });
    if (!reading.value && !isBusy()) view.focus();
    view.requestMeasure();
  }

  /** 外部内容更新后映射保存的光标，避免回编辑时位置越界。 */
  function trackUpdate(update: ViewUpdate): void {
    if (update.docChanged && editSelection) editSelection = editSelection.map(update.changes);
  }

  /** 非活跃标签只保存不可变选区，不持有完整编辑器。 */
  function snapshot(): EditorReadingSnapshot { return { reading: reading.value, editSelection }; }

  /** 已访问笔记恢复各自阅读状态；首次打开只继承模式，不继承旧文档光标。 */
  function restore(saved?: EditorReadingSnapshot): void {
    if (saved) reading.value = saved.reading;
    editSelection = saved?.editSelection ?? null;
  }

  /** 切换笔记时丢弃旧文档光标，但保留用户选择的阅读模式。 */
  function resetSelection(): void { editSelection = null; }

  /** 处理文件操作状态变化，不改变用户选择的模式。 */
  function refresh(): void {
    getView()?.dispatch({ effects: compartment.reconfigure(configuration()) });
  }

  return { reading, extensions, toggle, trackUpdate, resetSelection, refresh, snapshot, restore };
}
