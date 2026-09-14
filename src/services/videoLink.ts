/**
 * B 站链接的轻量识别：只用于输入提示与一键填入。
 *
 * 真正的解析、短链跳转与合法性校验都在后端完成，这里不做业务判断。
 */
const BILIBILI_PATTERN = /(bilibili\.com|b23\.tv|BV[0-9A-Za-z]{10}|(^|[^0-9A-Za-z])av\d+)/i;

/** 文本中是否包含 B 站视频链接或裸号。 */
export function looksLikeBilibili(text: string): boolean {
  return BILIBILI_PATTERN.test(text.trim());
}

/** 摘出第一个链接片段；找不到时返回空串。 */
export function extractLink(text: string): string {
  const match = text.match(/https?:\/\/[^\s。，、）】｝〉》"']+/i);
  if (match) return match[0].replace(/[.,;:!?/]+$/, "");
  const bare = text.trim().split(/\s+/)[0] ?? "";
  return looksLikeBilibili(bare) ? bare.replace(/[。，、）】]+$/, "") : "";
}