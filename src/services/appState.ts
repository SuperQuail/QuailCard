import * as backend from "../api/backend";
import * as uiStore from "./stores/uiStore";
import * as vaultStore from "./stores/vaultStore";
import * as noteStore from "./stores/noteStore";
import * as cardStore from "./stores/cardStore";
import * as reviewStore from "./stores/reviewStore";
import * as providerStore from "./stores/providerStore";

/**
 * 应用编排层：组合六个域 store，并承载跨域用例（初始化、打开 Vault、打开笔记等）。
 * 组件优先直接消费域 store；useAppState() 仅作为过渡期兼容门面保留。
 */

/** 启动时加载应用数据并分发到各域 store。 */
export async function initialize(): Promise<void> {
  if (uiStore.initialized.value || uiStore.loading.value) {
    return;
  }
  uiStore.loading.value = true;
  uiStore.errorMessage.value = "";
  try {
    const [data, status] = await Promise.all([backend.getBootstrapData(), backend.getVaultStatus()]);
    vaultStore.vaultPath.value = data.vaultPath;
    vaultStore.recentVaults.value = data.recentVaults;
    noteStore.notes.value = data.notes;
    providerStore.providers.value = data.providers;
    providerStore.activeProviderId.value = data.activeProviderId;
    reviewStore.studyStats.value = data.studyStats;
    reviewStore.aiGradingEnabled.value = data.aiGradingEnabled;
    uiStore.fontSize.value = data.fontSize;
    vaultStore.vaultStatus.value = status;
    await vaultStore.loadVaultConfig();
    uiStore.applyFontSize();
    uiStore.initialized.value = true;
  } catch (error) {
    uiStore.errorMessage.value = backend.resolveErrorMessage(error);
  } finally {
    uiStore.loading.value = false;
  }
}

/** 选择 Vault：打开后加载笔记与统计。 */
export async function openVault(path: string): Promise<void> {
  await vaultStore.openVault(path);
  await noteStore.refreshNotes();
}

/** 离开当前 Vault 回到选择页：清空各域残留状态。 */
export function leaveVault(): void {
  vaultStore.clearVault();
  noteStore.resetForVaultLeave();
  cardStore.clearActiveCards();
}

/** 窗口聚焦时重扫 Vault：刷新笔记列表并重载当前笔记内容。 */
export async function rescanVault(): Promise<void> {
  if (!vaultStore.vaultPath.value) {
    return;
  }
  try {
    await vaultStore.rescanVault();
    await noteStore.refreshNotes();
    await noteStore.reloadActiveContent();
  } catch (error) {
    uiStore.errorMessage.value = backend.resolveErrorMessage(error);
  }
}

/** 打开笔记：加载正文（含索引重建）后加载其卡片。 */
export async function selectNote(path: string): Promise<void> {
  await noteStore.selectNote(path);
  await cardStore.loadActiveCards(path);
}

/** 新建笔记文件并打开（正文与卡片一起就绪）。 */
export async function createNoteFile(folder: string, title: string, body: string): Promise<void> {
  await noteStore.createNoteFile(folder, title, body);
  if (noteStore.activeNotePath.value) {
    await cardStore.loadActiveCards(noteStore.activeNotePath.value);
  }
}

/** 删除笔记：当前笔记被删时回退并同步卡片区。 */
export async function deleteNoteFile(path: string): Promise<void> {
  await noteStore.deleteNoteFile(path);
  if (noteStore.activeNotePath.value) {
    await cardStore.loadActiveCards(noteStore.activeNotePath.value);
  } else {
    cardStore.clearActiveCards();
  }
}

/** 删除文件夹：当前笔记受影响时清空卡片区。 */
export async function deleteFolder(path: string): Promise<void> {
  await noteStore.deleteFolder(path);
  if (!noteStore.activeNotePath.value) {
    cardStore.clearActiveCards();
  }
}

/** 兼容门面：聚合各域 store 与跨域用例，保持既有组件的扁平调用方式。 */
export function useAppState() {
  return {
    ...uiStore,
    ...vaultStore,
    ...noteStore,
    ...cardStore,
    ...reviewStore,
    ...providerStore,
    initialize,
    openVault,
    leaveVault,
    rescanVault,
    selectNote,
    createNoteFile,
    deleteNoteFile,
    deleteFolder,
  };
}
