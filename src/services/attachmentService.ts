import * as backend from "../api/backend";
import type { ImportedAttachment } from "../domain/types";

const dataUrlCache = new Map<string, string>();
const inflight = new Map<string, Promise<string>>();
let cacheGeneration = 0;

/** 组合笔记与引用，避免不同目录下的同名图片碰撞。 */
function attachmentKey(notePath: string, source: string): string {
  return `${notePath}\u0000${source}`;
}

/** 将浏览器文件编码为后端所需的纯 Base64。 */
export async function fileToBase64(file: File): Promise<string> {
  const bytes = new Uint8Array(await file.arrayBuffer());
  let binary = "";
  const chunkSize = 0x8000;
  for (let offset = 0; offset < bytes.length; offset += chunkSize) {
    binary += String.fromCharCode(...bytes.subarray(offset, offset + chunkSize));
  }
  return btoa(binary);
}

/** 导入单个图片文件，文件校验由编辑器交互层负责。 */
export async function importImageAttachment(notePath: string, file: File): Promise<ImportedAttachment> {
  return backend.importNoteAttachment({
    notePath,
    fileName: file.name,
    mimeType: file.type,
    dataBase64: await fileToBase64(file),
  });
}

/** 读取附件并缓存可直接展示的 Data URL，同时合并并发请求。 */
export function resolveAttachmentDataUrl(notePath: string, source: string): Promise<string> {
  const key = attachmentKey(notePath, source);
  const cached = dataUrlCache.get(key);
  if (cached) {
    return Promise.resolve(cached);
  }
  const pending = inflight.get(key);
  if (pending) {
    return pending;
  }
  const generation = cacheGeneration;
  const request = backend.readNoteAttachment(notePath, source).then((image) => {
    const dataUrl = `data:${image.mimeType};base64,${image.dataBase64}`;
    if (generation === cacheGeneration) {
      dataUrlCache.set(key, dataUrl);
    }
    return dataUrl;
  }).finally(() => {
    if (inflight.get(key) === request) {
      inflight.delete(key);
    }
  });
  inflight.set(key, request);
  return request;
}

/** Vault 切换时清除图片结果，并使旧请求不能回填缓存。 */
export function clearAttachmentCache(): void {
  cacheGeneration += 1;
  dataUrlCache.clear();
  inflight.clear();
}
