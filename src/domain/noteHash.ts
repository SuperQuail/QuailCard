/** 与 Rust 契约一致：LF 正文的 UTF-8 SHA-256 小写十六进制。 */
export async function noteContentHash(content: string): Promise<string> {
  const digest = await crypto.subtle.digest("SHA-256", new TextEncoder().encode(content.replace(/\r\n/g, "\n")));
  return Array.from(new Uint8Array(digest), (byte) => byte.toString(16).padStart(2, "0")).join("");
}
