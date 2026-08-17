import type { AttachmentImage, ImportNoteAttachmentInput, ImportedAttachment, VaultConfig } from "../../domain/types";
import { vaultPath } from "./state";

const configs = new Map<string, VaultConfig>();
const images = new Map<string, AttachmentImage>();
let imageSequence = 0;

/** 取得当前演示 Vault 的稳定内存键。 */
function currentVaultKey(): string {
  return vaultPath ?? "__no_vault__";
}

/** 以演示级路径归一化解析点号段，不复制后端沙箱规则。 */
function normalizePath(path: string): string {
  const parts: string[] = [];
  for (const part of path.replace(/\\/g, "/").split("/")) {
    if (!part || part === ".") {
      continue;
    }
    if (part === "..") {
      parts.pop();
    } else {
      parts.push(part);
    }
  }
  return parts.join("/");
}

/** 计算从笔记目录到附件的 Markdown 相对路径。 */
function relativeToNote(notePath: string, target: string): string {
  const from = normalizePath(notePath).split("/").slice(0, -1);
  const to = normalizePath(target).split("/");
  while (from.length && to.length && from[0] === to[0]) {
    from.shift();
    to.shift();
  }
  return [...from.map(() => ".."), ...to].join("/");
}

/** 读取当前演示 Vault 的附件目录配置。 */
export function getVaultConfig(): VaultConfig {
  return configs.get(currentVaultKey()) ?? { attachmentFolder: "attachments" };
}

/** 保存当前演示 Vault 的附件目录配置。 */
export function setAttachmentFolder(attachmentFolder: string): VaultConfig {
  const config = { attachmentFolder: attachmentFolder.trim() || "attachments" };
  configs.set(currentVaultKey(), config);
  return config;
}

/** 将演示图片存入当前 Vault 的内存附件表。 */
export function importNoteAttachment(input: ImportNoteAttachmentInput): ImportedAttachment {
  const baseName = input.fileName.replace(/^.*[\\/]/, "").replace(/[^\p{L}\p{N}._-]+/gu, "-") || "image.png";
  const dot = baseName.lastIndexOf(".");
  const uniqueName = dot > 0
    ? `${baseName.slice(0, dot)}-${++imageSequence}${baseName.slice(dot)}`
    : `${baseName}-${++imageSequence}`;
  const target = normalizePath(`${getVaultConfig().attachmentFolder}/${uniqueName}`);
  images.set(`${currentVaultKey()}\u0000${target}`, { mimeType: input.mimeType, dataBase64: input.dataBase64 });
  return { markdownPath: relativeToNote(input.notePath, target) };
}

/** 按笔记位置解析并读取演示图片。 */
export function readNoteAttachment(notePath: string, source: string): AttachmentImage {
  const noteDirectory = normalizePath(notePath).split("/").slice(0, -1).join("/");
  const target = normalizePath(`${noteDirectory}/${source}`);
  const image = images.get(`${currentVaultKey()}\u0000${target}`);
  if (!image) {
    throw new Error("找不到图片附件");
  }
  return image;
}
