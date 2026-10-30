/** 后端命令错误码到兜底文案的映射：仅在 message 缺失时使用，避免把原始对象抛给用户。 */
const ERROR_CODE_FALLBACK: Record<string, string> = {
  TRASH_UNAVAILABLE: "无法移到回收站",
  CLIPBOARD_ERROR: "无法写入剪贴板",
  SAVE_ERROR: "保存图片失败",
  IMAGE_DECODE_ERROR: "图片无法解码",
};

/** 提取后端命令错误码；非结构化错误没有 code 时返回空串。 */
export function resolveErrorCode(error: unknown): string {
  if (typeof error === "object" && error !== null && "code" in error) {
    return String((error as { code: unknown }).code);
  }
  return "";
}

/** 从未知错误对象中提取安全消息（后端 DTO 只带 code + message）。 */
export function resolveError(error: unknown): string {
  if (typeof error === "object" && error !== null) {
    if ("message" in error) {
      return String(error.message);
    }
    const fallback = ERROR_CODE_FALLBACK[resolveErrorCode(error)];
    if (fallback) {
      return fallback;
    }
  }
  return String(error);
}
