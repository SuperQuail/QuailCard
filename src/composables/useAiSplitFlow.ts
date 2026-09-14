import { computed, getCurrentScope, onScopeDispose, watch } from "vue";
import * as backend from "../api/backend";
import type { CardSelection } from "../domain/types";
import { noteContentHash } from "../domain/noteHash";
import { createAiSplitService } from "../services/aiSplitService";
import type { AiSplitSnapshot } from "../services/aiSplitTypes";
import { activeNoteCards } from "../services/stores/cardStore";
import { activeNoteContent, activeNotePath, findNote, lastNotePathChange, notePersistence, notes } from "../services/stores/noteStore";
import { activeProviderId, providers } from "../services/stores/providerStore";
import { studyStats } from "../services/stores/reviewStore";
import { vaultPath } from "../services/stores/vaultStore";

/** 拆卡编排只读取所需域状态，并固定打开时的 Vault、笔记及编辑器选区。 */
export function useAiSplitFlow(options: { showToast: (message: string) => void; getSelection?: () => CardSelection | null }) {
  let vaultRevision = 0;
  let previousVault = vaultPath.value;
  const providerConfigured = computed(() => providers.value.some((provider) =>
    provider.id === activeProviderId.value && Boolean(provider.model.trim()) && (provider.hasApiKey || provider.hasCredential)));

  /** 保存完成后读取原始目标，正文变化时保留草稿并拒绝提交。 */
  async function verifySnapshot(snapshot: AiSplitSnapshot): Promise<string> {
    assertIdentity(snapshot);
    await notePersistence.flush(snapshot.notePath);
    assertIdentity(snapshot);
    const file = await backend.readNote(snapshot.notePath);
    assertIdentity(snapshot);
    const draft = notePersistence.states.get(snapshot.notePath)?.content;
    if (file.content.replace(/\r\n/g, "\n") !== snapshot.noteContent || (draft !== undefined && draft.replace(/\r\n/g, "\n") !== snapshot.noteContent)) {
      throw new Error("笔记正文已变化，草稿已保留，请关闭后重新生成");
    }
    return noteContentHash(snapshot.noteContent);
  }

  /** 检查原笔记身份，不能把草稿写入换库后的同名文件。 */
  function assertIdentity(snapshot: AiSplitSnapshot): void {
    if (vaultPath.value !== snapshot.vaultPath) throw new Error("Vault 已变化，请关闭后重新生成");
    if (!findNote(snapshot.notePath)) throw new Error("原笔记已删除或重命名，请关闭后重新生成");
  }

  /** 卡片及摘要同步失败单独报告，不能让用户误以为采纳未提交。 */
  async function refresh(snapshot: AiSplitSnapshot): Promise<void> {
    if (vaultPath.value !== snapshot.vaultPath) return;
    const revision = vaultRevision;
    const originalNotes = notes.value;
    const originalCards = activeNoteCards.value;
    const originalStats = studyStats.value;
    const originalPathChange = lastNotePathChange.value;
    const updates = await Promise.allSettled([backend.listNoteCards(snapshot.notePath), backend.listNotes(), backend.getStudyStats()]);
    if (vaultRevision !== revision || vaultPath.value !== snapshot.vaultPath) return;
    const [cards, summaries, stats] = updates;
    if (cards.status === "fulfilled" && activeNotePath.value === snapshot.notePath && activeNoteCards.value === originalCards) activeNoteCards.value = cards.value;
    if (summaries.status === "fulfilled" && notes.value === originalNotes && lastNotePathChange.value === originalPathChange) notes.value = summaries.value;
    if (stats.status === "fulfilled" && studyStats.value === originalStats) studyStats.value = stats.value;
    const failure = updates.find((update) => update.status === "rejected");
    if (failure?.status === "rejected") throw failure.reason;
  }

  const service = createAiSplitService({
    providerReady: () => providerConfigured.value, verifySnapshot, refresh,
    startGeneration: backend.startGeneration, getGenerationStatus: backend.getGenerationStatus,
    cancelGeneration: backend.cancelGeneration, adoptCards: backend.adoptCards, showToast: options.showToast,
  });

  /** 从 CodeMirror 捕获选区，忽略对话框或其他面板中的 DOM 选中文字。 */
  function openAiSplit(): void {
    const note = findNote(activeNotePath.value);
    if (!note || !vaultPath.value) return;
    const selection = options.getSelection?.();
    const noteContent = activeNoteContent.value.replace(/\r\n/g, "\n");
    const source = selection?.notePath === note.path ? selection.source : null;
    const validSelection = source?.excerpt.trim() && noteContent.slice(source.from, source.to) === source.excerpt ? source : null;
    service.open({
      vaultPath: vaultPath.value, notePath: note.path, noteTitle: note.title, noteContent,
      kind: activeNoteCards.value[0]?.kind === "vocabulary" ? "vocabulary" : "qa", selection: validSelection,
    });
  }

  /** 原笔记消失或 Vault 变化立即使会话失效，普通笔记切换不影响来源。 */
  const stopIdentityWatch = watch([vaultPath, () => notes.value.map((note) => note.path), lastNotePathChange], () => {
    if (vaultPath.value !== previousVault) { previousVault = vaultPath.value; ++vaultRevision; }
    const snapshot = service.state.value.snapshot;
    if (!snapshot) return;
    try { assertIdentity(snapshot); } catch (error) { service.invalidate(backend.resolveErrorMessage(error)); }
  }, { flush: "sync" });

  /** 页面卸载清理轮询与后端任务，避免隐藏任务继续生成。 */
  function dispose(): void { stopIdentityWatch(); service.close(); }
  if (getCurrentScope()) onScopeDispose(dispose);

  return {
    aiSplit: service.state, providerConfigured, openAiSplit, closeAiSplit: service.close,
    startAiSplit: service.start, stopAiSplit: service.stop, setAiSplitScope: service.setScope,
    setAiSplitCount: service.setCount, toggleAiDraft: service.toggleAccepted, removeAiDraft: service.removeDraft,
    handleAiSplitAdopt: service.adopt, dispose,
  };
}
