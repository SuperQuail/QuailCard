import { afterEach, beforeEach, describe, expect, test, vi } from "vitest";
import { effectScope, type EffectScope } from "vue";
import * as backend from "../api/backend";
import { captureCardSource } from "../domain/cardSource";
import { activeNoteCards } from "../services/stores/cardStore";
import { activeNoteContent, activeNotePath, notePersistence, notes, updateNoteDraft } from "../services/stores/noteStore";
import { activeProviderId, providers } from "../services/stores/providerStore";
import { vaultPath } from "../services/stores/vaultStore";
import { useAiSplitFlow } from "./useAiSplitFlow";

vi.mock("../api/backend", () => ({
  readNote: vi.fn(), writeNote: vi.fn(), startGeneration: vi.fn(), getGenerationStatus: vi.fn(), cancelGeneration: vi.fn(),
  adoptCards: vi.fn(), listNoteCards: vi.fn(), listNotes: vi.fn(), getStudyStats: vi.fn(),
  resolveErrorMessage: (error: Error) => error.message,
}));
vi.mock("../domain/noteHash", () => ({ noteContentHash: vi.fn().mockResolvedValue("hash") }));

const scopes: EffectScope[] = [];
const original = "前文😀\n选中文本";

/** 用真实 Vue effect scope 执行用例，测试结束时释放其监听器。 */
function setup(getSelection?: Parameters<typeof useAiSplitFlow>[0]["getSelection"]) {
  const scope = effectScope();
  scopes.push(scope);
  const showToast = vi.fn();
  const flow = scope.run(() => useAiSplitFlow({ showToast, getSelection }))!;
  return { flow, showToast };
}

beforeEach(() => {
  vi.useFakeTimers();
  vi.clearAllMocks();
  notePersistence.remove();
  vaultPath.value = "Vault-A";
  activeNotePath.value = "原笔记.md";
  activeNoteContent.value = original;
  notes.value = ["原笔记.md", "另一篇.md"].map((path) => ({ path, title: path, tagsJson: "[]", cardCount: 0, dueCount: 0, mtime: 1 }));
  activeNoteCards.value = [];
  notePersistence.register("原笔记.md", original);
  providers.value = [{ id: "active", name: "AI", model: "model", models: [{ id: "model", name: "model", contextWindow: null, maxOutputTokens: null }], shortCode: "AI", protocol: "openai", baseUrl: "https://example.test", hasApiKey: true, hasCredential: false, authType: null, oauthAccountId: null, providerType: "api", supportsVision: true, status: "connected" }];
  activeProviderId.value = "active";
  vi.mocked(backend.readNote).mockResolvedValue({ path: "原笔记.md", title: "原笔记", content: original, mtime: 1 });
  vi.mocked(backend.writeNote).mockResolvedValue(2);
  vi.mocked(backend.startGeneration).mockResolvedValue({ taskId: "task" });
  vi.mocked(backend.getGenerationStatus).mockResolvedValue({ taskId: "task", state: "completed", phase: "validating", generatedCount: 1, result: { cards: [{ draftId: "draft", fields: { front: "问题", back: "答案" }, source: null }], warnings: [] }, error: null });
  vi.mocked(backend.cancelGeneration).mockResolvedValue({ taskId: "task", state: "cancelled", phase: "preparing", generatedCount: 0, result: { cards: [], warnings: [] }, error: null });
  vi.mocked(backend.adoptCards).mockResolvedValue({ addedIds: ["draft"], existingIds: [], duplicateIds: [] });
  vi.mocked(backend.listNoteCards).mockResolvedValue([]);
  vi.mocked(backend.listNotes).mockResolvedValue(notes.value);
  vi.mocked(backend.getStudyStats).mockResolvedValue({ dueCount: 0, totalCards: 1, weeklyCompletedCount: 0, weeklyCompletionRate: null });
});

afterEach(() => {
  for (const scope of scopes.splice(0)) scope.stop();
  notePersistence.remove();
  vi.clearAllTimers();
  vi.useRealTimers();
});

describe("拆卡来源与保存协调", () => {
  test("默认使用 CodeMirror 选区，换笔记后仍生成并采纳到原笔记", async () => {
    const source = captureCardSource(original, 5, original.length);
    const { flow } = setup(() => ({ notePath: "原笔记.md", source }));
    flow.openAiSplit();
    activeNotePath.value = "另一篇.md";
    activeNoteContent.value = "其他正文";
    expect(flow.aiSplit.value.scope).toBe("selection");
    await flow.startAiSplit();
    expect(backend.startGeneration).toHaveBeenCalledWith(expect.objectContaining({ sourceText: "选中文本", context: { vaultPath: "Vault-A", notePath: "原笔记.md", noteHash: "hash", selection: source } }));
    await vi.advanceTimersByTimeAsync(500);
    await flow.handleAiSplitAdopt();
    expect(backend.adoptCards).toHaveBeenCalledWith(expect.objectContaining({ notePath: "原笔记.md", expectedVaultPath: "Vault-A", expectedNoteHash: "hash" }));
    expect(activeNotePath.value).toBe("另一篇.md");
  });

  test("开始前等待笔记写盘，不使用浏览器外部 DOM 选区", async () => {
    const events: string[] = [];
    updateNoteDraft("原笔记.md", "新正文");
    vi.mocked(backend.writeNote).mockImplementation(async () => { events.push("write"); return 2; });
    vi.mocked(backend.readNote).mockImplementation(async () => { events.push("read"); return { path: "原笔记.md", title: "原笔记", content: "新正文", mtime: 2 }; });
    vi.mocked(backend.startGeneration).mockImplementation(async () => { events.push("start"); return { taskId: "task" }; });
    const { flow } = setup(() => ({ notePath: "其他.md", source: captureCardSource("外部选区", 0, 4) }));
    flow.openAiSplit();
    expect(flow.aiSplit.value.scope).toBe("note");
    await flow.startAiSplit();
    expect(events).toEqual(["write", "read", "start"]);
    expect(backend.startGeneration).toHaveBeenCalledWith(expect.objectContaining({ sourceText: "新正文" }));
  });

  test("正文改变时采纳拒绝，草稿仍然保留", async () => {
    const { flow, showToast } = setup();
    flow.openAiSplit();
    await flow.startAiSplit();
    await vi.advanceTimersByTimeAsync(500);
    updateNoteDraft("原笔记.md", "改过的正文");
    vi.mocked(backend.readNote).mockResolvedValue({ path: "原笔记.md", title: "原笔记", content: "改过的正文", mtime: 2 });
    await flow.handleAiSplitAdopt();
    expect(backend.adoptCards).not.toHaveBeenCalled();
    expect(flow.aiSplit.value.drafts).toHaveLength(1);
    expect(flow.aiSplit.value.open).toBe(true);
    expect(showToast).toHaveBeenLastCalledWith(expect.stringContaining("正文已变化"));
  });

  test.each(["rename", "vault"])("%s 使来源永久失效", async (change) => {
    const { flow } = setup();
    flow.openAiSplit();
    if (change === "rename") notes.value = [{ ...notes.value[0], path: "改名.md" }];
    else { vaultPath.value = "Vault-B"; vaultPath.value = "Vault-A"; }
    await flow.startAiSplit();
    expect(flow.aiSplit.value.invalidReason).not.toBe("");
    expect(backend.startGeneration).not.toHaveBeenCalled();
  });

  test("其他供应商有凭据不能使未配置的活动供应商就绪", async () => {
    const { flow } = setup();
    activeProviderId.value = "missing";
    flow.openAiSplit();
    await flow.startAiSplit();
    expect(flow.providerConfigured.value).toBe(false);
    expect(backend.startGeneration).not.toHaveBeenCalled();
  });

  test.each(["rename", "vault"])("采纳后的迟到刷新不会覆盖 %s 后的新笔记列表", async (change) => {
    const { flow } = setup();
    flow.openAiSplit();
    await flow.startAiSplit();
    await vi.advanceTimersByTimeAsync(500);
    const oldNotes = [...notes.value];
    let release!: (value: typeof oldNotes) => void;
    vi.mocked(backend.listNotes).mockReturnValueOnce(new Promise((resolve) => { release = resolve; }));
    const adopting = flow.handleAiSplitAdopt();
    for (let i = 0; i < 12; i += 1) await Promise.resolve();
    expect(flow.aiSplit.value.open).toBe(false);
    if (change === "vault") { vaultPath.value = "Vault-B"; vaultPath.value = "Vault-A"; }
    notes.value = [{ ...oldNotes[0], path: "新笔记.md" }];
    release(oldNotes);
    await adopting;
    expect(notes.value[0].path).toBe("新笔记.md");
  });
});
