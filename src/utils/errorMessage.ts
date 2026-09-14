/** 从未知错误对象中提取安全消息（后端 DTO 只带 code + message）。 */
export function resolveError(error: unknown): string {
  if (typeof error === "object" && error !== null && "message" in error) {
    return String(error.message);
  }
  return String(error);
}
