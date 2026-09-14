import { Compartment, EditorState } from "@codemirror/state";
import { EditorView } from "@codemirror/view";
import { afterEach, expect, test, vi } from "vitest";
import { importImageAttachment } from "../services/attachmentService";
import type { ImportedAttachment } from "../domain/types";
import { imageTransferExtension } from "./imageTransfer";

vi.mock("../services/attachmentService", () => ({ importImageAttachment: vi.fn() }));
const views: EditorView[] = [];
/** 释放编辑器并清理模拟服务，避免异步请求跨用例。 */
afterEach(() => { views.splice(0).forEach((view) => view.destroy()); vi.resetAllMocks(); });
/** 挂载真实 DOM 事件扩展，验证只读检查发生在导入服务调用之前。 */
function editor(readOnly: boolean) {
  const lock = new Compartment();
  const view = new EditorView({ parent: document.body, state: EditorState.create({
    doc: "正文", extensions: [lock.of(EditorState.readOnly.of(readOnly)), imageTransferExtension(() => "笔记.md", vi.fn())],
  }) });
  views.push(view);
  return { view, lock };
}
/** jsdom 没有完整 DataTransfer；仅提供扩展实际使用的文件列表。 */
function imageEvent(type: "paste" | "drop"): Event {
  const event = new Event(type, { bubbles: true, cancelable: true });
  Object.defineProperty(event, type === "paste" ? "clipboardData" : "dataTransfer", {
    value: { files: [new File(["png"], "图片.png", { type: "image/png" })] },
  });
  return event;
}

/** 不仅禁止正文变化，也禁止只读模式下启动附件持久化。 */
test.each(["paste", "drop"] as const)("只读时阻止 %s 附件导入", (type) => {
  const { view } = editor(true);
  const event = imageEvent(type);
  view.contentDOM.dispatchEvent(event);
  expect(event.defaultPrevented).toBe(true);
  expect(importImageAttachment).not.toHaveBeenCalled();
  expect(view.state.doc.toString()).toBe("正文");
});

/** 粘贴已开始后切入阅读模式，迟到响应不得再插入图片 Markdown。 */
test("导入途中切换只读后不再写入正文", async () => {
  let finish!: (attachment: ImportedAttachment) => void;
  vi.mocked(importImageAttachment).mockImplementation(() => new Promise((resolve) => { finish = resolve; }));
  const { view, lock } = editor(false);
  view.contentDOM.dispatchEvent(imageEvent("paste"));
  expect(importImageAttachment).toHaveBeenCalledTimes(1);
  view.dispatch({ effects: lock.reconfigure(EditorState.readOnly.of(true)) });
  finish({ markdownPath: "attachments/图片.png" });
  await Promise.resolve();
  expect(view.state.doc.toString()).toBe("正文");
  expect(view.dom.querySelector(".qc-upload-marker")).toBeNull();
});

/** 退出阅读后原有图片插入仍正常，不能把所有导入都禁掉。 */
test("编辑状态仍可粘贴图片", async () => {
  vi.mocked(importImageAttachment).mockResolvedValue({ markdownPath: "attachments/图片.png" });
  const { view } = editor(false);
  view.contentDOM.dispatchEvent(imageEvent("paste"));
  await Promise.resolve();
  expect(importImageAttachment).toHaveBeenCalledTimes(1);
  expect(view.state.doc.toString()).toContain("![图片](attachments/图片.png)");
});
