import * as backend from "../api/backend";

/**
 * 图片附件的对外动作：复制、另存、在文件管理器中定位。
 *
 * 只做用例转发，路径解析与校验全部留在后端；展示层不接触文件系统。
 */

/** 把笔记内图片作为位图写入系统剪贴板。 */
export function copyAttachmentImage(notePath: string, source: string): Promise<void> {
  return backend.copyNoteAttachmentImage(notePath, source);
}

/** 另存图片到用户选择的位置；返回保存路径，取消时返回 null。 */
export function saveAttachmentAs(notePath: string, source: string): Promise<string | null> {
  return backend.saveNoteAttachmentAs(notePath, source);
}

/** 在系统文件管理器中定位并选中图片文件。 */
export function revealAttachment(notePath: string, source: string): Promise<void> {
  return backend.revealNoteAttachment(notePath, source);
}
