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

/**
 * 解析图片在演示 Vault 内的目标路径：笔记目录 + Markdown 原始地址。
 *
 * 三个图片动作与读取共用这一处解析，避免各命令各写一套路径拼接
 * （演示层只做归一化，不复刻后端的沙箱与安全规则）。
 */
function resolveAttachmentTarget(notePath: string, source: string): string {
  const noteDirectory = normalizePath(notePath).split("/").slice(0, -1).join("/");
  return normalizePath(`${noteDirectory}/${source}`);
}

/** 按笔记位置解析并读取演示图片。 */
export function readNoteAttachment(notePath: string, source: string): AttachmentImage {
  const target = resolveAttachmentTarget(notePath, source);
  const image = images.get(`${currentVaultKey()}\u0000${target}`);
  if (!image) {
    throw new Error("找不到图片附件");
  }
  return image;
}

/**
 * 复制图片到系统剪贴板。
 *
 * 演示环境不写真实剪贴板（浏览器演示没有位图剪贴板权限），
 * 但仍按同一解析确认图片存在，保证失败路径与真机一致。
 */
export function copyNoteAttachmentImage(notePath: string, source: string): void {
  readNoteAttachment(notePath, source);
}

/** 演示另存目录：不弹系统保存对话框，避免演示流程卡在原生窗口。 */
const DEMO_DOWNLOAD_DIR = "C:/Users/demo/Downloads";

/**
 * 另存图片。
 *
 * 演示环境不弹对话框，直接返回仿真的下载路径；镜像后端语义：
 * 只有用户取消才返回 null，这里永不取消。
 */
export function saveNoteAttachmentAs(notePath: string, source: string): string {
  readNoteAttachment(notePath, source);
  const target = resolveAttachmentTarget(notePath, source);
  const fileName = target.slice(target.lastIndexOf("/") + 1) || "image.png";
  return `${DEMO_DOWNLOAD_DIR}/${fileName}`;
}

/**
 * 在系统文件管理器中定位图片。
 *
 * 浏览器演示无法打开文件管理器，确认图片存在后静默成功即可。
 */
export function revealNoteAttachment(notePath: string, source: string): void {
  readNoteAttachment(notePath, source);
}
