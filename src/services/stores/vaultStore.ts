import { ref } from "vue";
import * as backend from "../../api/backend";
import type { VaultStatus } from "../../domain/types";

/**
 * Vault 域 store：当前保险库路径、最近列表与加密状态。
 * 只管理 Vault 自身的状态；打开后的笔记加载由编排层协调 noteStore 完成。
 */
export const vaultPath = ref<string | null>(null);
export const recentVaults = ref<string[]>([]);
export const vaultStatus = ref<VaultStatus | null>(null);

/** 打开 Vault：后端登记后更新路径并置顶最近列表。 */
export async function openVault(path: string): Promise<void> {
  const root = await backend.openVault(path);
  vaultPath.value = root;
  recentVaults.value = [root, ...recentVaults.value.filter((item) => item !== root)].slice(0, 10);
}

/** 离开当前 Vault 回到选择页（笔记清理由编排层协调 noteStore 完成）。 */
export function clearVault(): void {
  vaultPath.value = null;
}

/** 设置保险库密码并刷新加密状态。 */
export async function setVaultPassword(password: string): Promise<void> {
  vaultStatus.value = await backend.setVaultPassword(password);
}

/** 触发后端重扫：窗口聚焦时检测外部修改。 */
export async function rescanVault(): Promise<void> {
  await backend.rescanVault();
}
