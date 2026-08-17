import type { Extension } from "@codemirror/state";
import { EditorSelection, StateEffect, StateField } from "@codemirror/state";
import { Decoration, type DecorationSet, EditorView, WidgetType } from "@codemirror/view";
import { importImageAttachment } from "../services/attachmentService";

const IMAGE_TYPES = new Set(["image/png", "image/jpeg", "image/webp"]);
const MAX_IMAGE_BYTES = 10 * 1024 * 1024;
const MAX_IMAGE_COUNT = 8;
let transferSequence = 0;

interface UploadMarkerSpec {
  id: string;
  position: number;
  side: -1 | 1;
}

const addUploadMarker = StateEffect.define<UploadMarkerSpec>({
  /** 文档与标记同时变化时，将待插入位置映射到新文档。 */
  map: (value, changes) => ({ ...value, position: changes.mapPos(value.position, value.side) }),
});
const removeUploadMarkers = StateEffect.define<Set<string>>();

/** 不产生可见内容的上传位置标记，随 CodeMirror 事务自动移动。 */
class UploadMarkerWidget extends WidgetType {
  constructor(readonly id: string) {
    super();
  }

  /** 相同上传标记可以复用空 DOM。 */
  eq(other: UploadMarkerWidget): boolean {
    return other.id === this.id;
  }

  /** 标记只用于定位，不干扰编辑器排版。 */
  toDOM(): HTMLElement {
    const marker = document.createElement("span");
    marker.className = "qc-upload-marker";
    return marker;
  }
}

const uploadMarkers = StateField.define<DecorationSet>({
  /** 上传标记初始为空。 */
  create: () => Decoration.none,
  /** 先映射已有位置，再处理本次新增和删除。 */
  update(markers, transaction) {
    let next = markers.map(transaction.changes);
    for (const effect of transaction.effects) {
      if (effect.is(addUploadMarker)) {
        const { id, position, side } = effect.value;
        next = next.update({ add: [Decoration.widget({ widget: new UploadMarkerWidget(id), side }).range(position)] });
      } else if (effect.is(removeUploadMarkers)) {
        next = next.update({
          filter: (_from, _to, decoration) => {
            const widget = decoration.spec.widget;
            return !(widget instanceof UploadMarkerWidget) || !effect.value.has(widget.id);
          },
        });
      }
    }
    return next;
  },
  provide: (field) => EditorView.decorations.from(field),
});

/** 从传输对象保留支持的图片，其他内容交还 CodeMirror 默认处理。 */
function imageFiles(transfer: DataTransfer | null): File[] {
  return transfer ? Array.from(transfer.files).filter((file) => IMAGE_TYPES.has(file.type)) : [];
}

/** 查找一个仍存活的上传标记当前位置。 */
function markerPosition(view: EditorView, id: string): number | null {
  let position: number | null = null;
  view.state.field(uploadMarkers).between(0, view.state.doc.length, (from, _to, decoration) => {
    const widget = decoration.spec.widget;
    if (widget instanceof UploadMarkerWidget && widget.id === id) {
      position = from;
    }
  });
  return position;
}

/** 删除一组上传位置标记。 */
function clearMarkers(view: EditorView, ids: string[]): void {
  view.dispatch({ effects: removeUploadMarkers.of(new Set(ids)) });
}

/** 将文件名转为不会破坏 Markdown 标签的简短替代文本。 */
function imageAlt(fileName: string): string {
  return fileName.replace(/\.[^.]+$/, "").replace(/[\[\]\\]/g, " ").trim() || "image";
}

/** 根据插入点两侧内容补齐换行，避免图片语法与正文粘连。 */
export function formatImageInsertion(document: string, from: number, to: number, markdown: string): string {
  const prefix = from > 0 && document[from - 1] !== "\n" ? "\n" : "";
  const suffix = to < document.length && document[to] !== "\n" ? "\n" : "";
  return `${prefix}${markdown}${suffix}`;
}

/** 依次导入图片并在原笔记仍打开时按文件顺序一次性插入。 */
async function importAndInsert(
  view: EditorView,
  files: File[],
  markerIds: { from: string; to: string },
  originNotePath: string,
  currentNotePath: () => string,
  onError: (message: string) => void,
): Promise<void> {
  const markdown: string[] = [];
  for (const file of files) {
    if (file.size > MAX_IMAGE_BYTES) {
      onError(`${file.name} 超过 10 MiB`);
      continue;
    }
    if (currentNotePath() !== originNotePath) {
      if (view.dom.isConnected) {
        clearMarkers(view, [markerIds.from, markerIds.to]);
      }
      return;
    }
    try {
      const imported = await importImageAttachment(originNotePath, file);
      markdown.push(`![${imageAlt(file.name)}](${imported.markdownPath})`);
    } catch {
      onError(`${file.name} 导入失败`);
    }
  }
  if (!view.dom.isConnected) {
    return;
  }
  if (!markdown.length || currentNotePath() !== originNotePath) {
    clearMarkers(view, [markerIds.from, markerIds.to]);
    return;
  }
  const markedFrom = markerPosition(view, markerIds.from);
  const markedTo = markerPosition(view, markerIds.to);
  if (markedFrom === null || markedTo === null) {
    clearMarkers(view, [markerIds.from, markerIds.to]);
    return;
  }
  const from = Math.min(markedFrom, markedTo);
  const to = Math.max(markedFrom, markedTo);
  const insert = formatImageInsertion(view.state.doc.toString(), from, to, markdown.join("\n"));
  view.dispatch({
    changes: { from, to, insert },
    effects: removeUploadMarkers.of(new Set([markerIds.from, markerIds.to])),
    selection: EditorSelection.cursor(from + insert.length),
    scrollIntoView: true,
  });
}

/** 创建图片粘贴与拖放扩展；纯文本事件返回 false 保持原行为。 */
export function imageTransferExtension(
  currentNotePath: () => string,
  onError: (message: string) => void,
): Extension {
  const handlers = EditorView.domEventHandlers({
    /** 图片粘贴在当前选区起点插入。 */
    paste(event, view) {
      const files = imageFiles(event.clipboardData);
      if (!files.length) {
        return false;
      }
      event.preventDefault();
      onError("");
      if (files.length > MAX_IMAGE_COUNT) {
        onError(`一次最多导入 ${MAX_IMAGE_COUNT} 张图片`);
      }
      const notePath = currentNotePath();
      const selection = view.state.selection.main;
      const markerIds = createMarkers(view, selection.from, selection.to);
      void importAndInsert(view, files.slice(0, MAX_IMAGE_COUNT), markerIds, notePath, currentNotePath, onError);
      return true;
    },
    /** 图片拖放按指针坐标插入，不使用当前光标位置。 */
    drop(event, view) {
      const files = imageFiles(event.dataTransfer);
      if (!files.length) {
        return false;
      }
      event.preventDefault();
      onError("");
      if (files.length > MAX_IMAGE_COUNT) {
        onError(`一次最多导入 ${MAX_IMAGE_COUNT} 张图片`);
      }
      const notePath = currentNotePath();
      const position = view.posAtCoords({ x: event.clientX, y: event.clientY }) ?? view.state.selection.main.from;
      const markerIds = createMarkers(view, position, position);
      void importAndInsert(view, files.slice(0, MAX_IMAGE_COUNT), markerIds, notePath, currentNotePath, onError);
      return true;
    },
  });
  return [uploadMarkers, handlers];
}

/** 在原选区两端创建可随后续编辑映射的位置标记。 */
function createMarkers(view: EditorView, from: number, to: number): { from: string; to: string } {
  const prefix = `attachment-${Date.now()}-${transferSequence++}`;
  const ids = { from: `${prefix}-from`, to: `${prefix}-to` };
  view.dispatch({
    effects: [
      addUploadMarker.of({ id: ids.from, position: from, side: 1 }),
      addUploadMarker.of({ id: ids.to, position: to, side: -1 }),
    ],
  });
  return ids;
}
