import { ref } from "vue";
import * as backend from "../../api/backend";
import type { DataLocations, VaultConfig, VaultStatus } from "../../domain/types";
import { clearAttachmentCache } from "../attachmentService";

/**
 * Vault 域 store：当前保险库路径、最近列表与加密状态。
 * 只管理 Vault 自身的状态；打开后的笔记加载由编排层协调 noteStore 完成。
 */
export const vaultPath = ref<string | null>(null);
export const recentVaults = ref<string[]>([]);
export const vaultStatus = ref<VaultStatus | null>(null);
export const vaultConfig = ref<VaultConfig>({ attachmentFolder: "attachments" });
export const attachmentFolderSaving = ref(false);
export const attachmentFolderStatus = ref("");
export const dataLocations = ref<DataLocations>({ cardsDir: null, configDir: "" });

/** 刷新数据位置信息；Vault 打开/离开后由编排层调用。 */
export async function loadDataLocations(): Promise<void> {
  dataLocations.value = await backend.getDataLocations();
}

/** 加载当前 Vault 的文件配置；无 Vault 时恢复界面默认值。 */
export async function loadVaultConfig(): Promise<void> {
  const originVault = vaultPath.value;
  const config = originVault
    ? await backend.getVaultConfig()
    : { attachmentFolder: "attachments" };
  if (vaultPath.value === originVault) {
    vaultConfig.value = config;
    attachmentFolderStatus.value = "";
  }
}

/** 打开 Vault：后端登记后更新路径并置顶最近列表。 */
export async function openVault(path: string): Promise<void> {
  clearAttachmentCache();
  attachmentFolderSaving.value = false;
  const root = await backend.openVault(path);
  vaultPath.value = root;
  recentVaults.value = [root, ...recentVaults.value.filter((item) => item !== root)].slice(0, 10);
  await loadVaultConfig();
}

/** 离开当前 Vault 回到选择页（笔记清理由编排层协调 noteStore 完成）。 */
export function clearVault(): void {
  clearAttachmentCache();
  attachmentFolderSaving.value = false;
  vaultPath.value = null;
  vaultConfig.value = { attachmentFolder: "attachments" };
  attachmentFolderStatus.value = "";
}

/** 保存当前 Vault 的附件目录并提供就地反馈。 */
export async function setAttachmentFolder(attachmentFolder: string): Promise<void> {
  const originVault = vaultPath.value;
  if (!originVault) {
    attachmentFolderStatus.value = "请先打开 Vault";
    return;
  }
  attachmentFolderSaving.value = true;
  attachmentFolderStatus.value = "";
  try {
    const config = await backend.setAttachmentFolder(attachmentFolder.trim());
    if (vaultPath.value === originVault) {
      vaultConfig.value = config;
      attachmentFolderStatus.value = "已保存";
    }
  } catch (error) {
    if (vaultPath.value === originVault) {
      attachmentFolderStatus.value = backend.resolveErrorMessage(error);
    }
  } finally {
    if (vaultPath.value === originVault) {
      attachmentFolderSaving.value = false;
    }
  }
}

/** 设置保险库密码并刷新加密状态。 */
export async function setVaultPassword(password: string): Promise<void> {
  vaultStatus.value = await backend.setVaultPassword(password);
}

/** 触发后端重扫：窗口聚焦时检测外部修改。 */
export async function rescanVault(): Promise<void> {
  await backend.rescanVault();
}
