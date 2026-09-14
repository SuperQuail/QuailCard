import type { AgentImage } from "../domain/agent";

/** 只消费剪贴板中的真实图片文件，普通文字粘贴继续由输入框处理。 */
export function clipboardImages(data: DataTransfer | null): File[] {
  if (!data) return [];
  const files = Array.from(data.files ?? []).filter(file => file.type.startsWith("image/"));
  if (files.length) return files;
  return Array.from(data.items ?? []).filter(item => item.kind === "file" && item.type.startsWith("image/"))
    .map(item => item.getAsFile()).filter((file): file is File => !!file);
}

/** 先完成整批预算校验再读取，失败不应留下半批附件。 */
export async function readAgentImages(files: File[], existing: AgentImage[]): Promise<AgentImage[]> {
  if (files.length + existing.length > 4) throw new Error("一次最多发送 4 张图片");
  let total = existing.reduce((size, image) => size + Math.floor(image.dataBase64.length * 3 / 4), 0);
  for (const file of files) {
    if (!["image/png", "image/jpeg", "image/webp"].includes(file.type)) throw new Error("仅支持 PNG、JPG 和 WebP 图片");
    if (!file.size || file.size > 5 * 1024 * 1024) throw new Error("单张图片大小必须在 5 MiB 以内");
    total += file.size;
  }
  if (total > 15 * 1024 * 1024) throw new Error("图片总大小不能超过 15 MiB");
  return Promise.all(files.map(readImage));
}

/** 浏览器读取剪贴板文件，不访问桌面文件系统或上传到外部服务。 */
function readImage(file: File): Promise<AgentImage> {
  return new Promise((resolve, reject) => {
    const reader = new FileReader();
    reader.onerror = () => reject(new Error("读取粘贴图片失败，请重试"));
    reader.onabort = () => reject(new Error("图片读取已取消"));
    reader.onload = () => resolve({ name: file.name || "粘贴图片.png", mimeType: file.type, dataBase64: String(reader.result).split(",")[1] });
    reader.readAsDataURL(file);
  });
}
