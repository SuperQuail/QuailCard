import { afterEach, beforeEach, expect, test, vi } from "vitest";
import * as backend from "../api/backend";
import { coordinateAgentWrite } from "./agentFiles";
import { activeNoteContent, activeNotePath, noteOperationBusy, notePersistence, notes } from "./stores/noteStore";
vi.mock("../api/backend", () => ({ writeNote: vi.fn(), readNote: vi.fn(), rescanVault: vi.fn(), listNotes: vi.fn(), getStudyStats: vi.fn(), resolveErrorMessage: (error: Error) => error.message }));
beforeEach(() => {
  vi.clearAllMocks(); notePersistence.remove(); noteOperationBusy.value = false;
  notes.value = [{ path: "a.md", title: "A", tagsJson: "[]", cardCount: 0, dueCount: 0, mtime: 1 }];
  activeNotePath.value = "a.md"; activeNoteContent.value = "旧文"; notePersistence.register("a.md", "旧文");
  vi.mocked(backend.writeNote).mockResolvedValue(2); vi.mocked(backend.listNotes).mockResolvedValue(notes.value);
  vi.mocked(backend.readNote).mockResolvedValue({ path: "a.md", title: "A", content: "Agent 新文", mtime: 3 });
});
afterEach(() => { notePersistence.remove(); });
test("先保存草稿，再握手写入，更新编辑器完成前保持只读", async () => {
  notePersistence.update("a.md", "用户草稿");
  const action = vi.fn(async () => {
    expect(noteOperationBusy.value).toBe(true);
    expect(backend.writeNote).toHaveBeenCalledWith("a.md", "用户草稿");
  });
  vi.mocked(backend.readNote).mockImplementation(async () => {
    expect(noteOperationBusy.value).toBe(true);
    return { path: "a.md", title: "A", content: "Agent 新文", mtime: 3 };
  });
  await coordinateAgentWrite(action);
  expect(action).toHaveBeenCalledOnce(); expect(activeNoteContent.value).toBe("Agent 新文");
  expect(notePersistence.states.get("a.md")?.savedContent).toBe("Agent 新文"); expect(noteOperationBusy.value).toBe(false);
});
test("草稿保存失败不能批准 Agent 写入，并保留未保存内容", async () => {
  notePersistence.update("a.md", "不能丢失的草稿"); vi.mocked(backend.writeNote).mockRejectedValue(new Error("磁盘失败"));
  const action = vi.fn(); await expect(coordinateAgentWrite(action)).rejects.toThrow("磁盘失败");
  expect(action).not.toHaveBeenCalled(); expect(notePersistence.states.get("a.md")?.content).toBe("不能丢失的草稿"); expect(noteOperationBusy.value).toBe(false);
});
