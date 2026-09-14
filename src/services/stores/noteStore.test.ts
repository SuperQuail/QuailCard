import { beforeEach, describe, expect, test, vi } from "vitest";
import * as backend from "../../api/backend";
import { activeNoteContent, activeNotePath, lastNotePathChange, noteOperationBusy, notePersistence, notes, renameFolder, renameNoteFile, updateNoteDraft, selectNote, resetForVaultLeave, clearActiveNote } from "./noteStore";

vi.mock("../../api/backend", () => ({ readNote: vi.fn(), syncNoteIndex: vi.fn(), writeNote: vi.fn(), renameNoteFile: vi.fn(), renameFolder: vi.fn(), listNotes: vi.fn(), getStudyStats: vi.fn(), resolveErrorMessage: String }));

beforeEach(() => {
  vi.resetAllMocks();
  notePersistence.remove();
  noteOperationBusy.value = false;
  lastNotePathChange.value = null;
  activeNotePath.value = "old/a.md";
  activeNoteContent.value = "初始";
  notes.value = [{ path: "old/a.md", title: "a", tagsJson: "[]", mtime: 1, cardCount: 2, dueCount: 1 }];
  notePersistence.register("old/a.md", "初始");
  vi.mocked(backend.writeNote).mockResolvedValue(2);
  vi.mocked(backend.getStudyStats).mockRejectedValue(new Error("统计暂不可用"));
});

/** 关闭最后一页不是删笔记，迟到读取也不应重新打开已经关闭的页面。 */
test("清空当前选择保留索引与草稿，并取消正在读取的笔记", async () => {
  let finish!: (file: Awaited<ReturnType<typeof backend.readNote>>) => void;
  vi.mocked(backend.readNote).mockReturnValue(new Promise(resolve => { finish = resolve; }));
  const opening = selectNote("old/a.md");
  clearActiveNote();
  finish({ path: "old/a.md", title: "a", content: "迟到正文", mtime: 1 });
  await opening;
  expect(activeNotePath.value).toBeNull();
  expect(activeNoteContent.value).toBe("");
  expect(notes.value).toHaveLength(1);
  expect(notePersistence.states.get("old/a.md")?.content).toBe("初始");
  expect(backend.writeNote).not.toHaveBeenCalled();
});

describe("重命名与保存协调", () => {
  test("先保存再改名，统计失败不阻止名称更新", async () => {
    const events: string[] = [];
    vi.mocked(backend.writeNote).mockImplementation(async () => { events.push("save"); return 2; });
    vi.mocked(backend.renameNoteFile).mockImplementation(async () => { events.push("rename"); return "old/z.md"; });
    vi.mocked(backend.listNotes).mockResolvedValue([{ ...notes.value[0], path: "old/z.md", title: "z" }]);
    updateNoteDraft("old/a.md", "刚输入");
    await renameNoteFile("old/a.md", "old/z.md");
    expect(events).toEqual(["save", "rename"]);
    expect(notes.value[0].title).toBe("z");
    expect(activeNotePath.value).toBe("old/z.md");
    expect(notePersistence.states.get("old/z.md")?.content).toBe("刚输入");
    expect(lastNotePathChange.value).toEqual({ oldPath: "old/a.md", newPath: "old/z.md" });
  });
  test("保存失败阻止改名，草稿仍然可用", async () => {
    vi.mocked(backend.writeNote).mockRejectedValue(new Error("保存失败"));
    updateNoteDraft("old/a.md", "未保存内容");
    await expect(renameNoteFile("old/a.md", "old/z.md")).rejects.toThrow("保存失败");
    expect(backend.renameNoteFile).not.toHaveBeenCalled();
    expect(activeNotePath.value).toBe("old/a.md");
    expect(notePersistence.states.get("old/a.md")?.content).toBe("未保存内容");
    expect(noteOperationBusy.value).toBe(false);
  });
  test("列表刷新失败时保留已成功的目录改名", async () => {
    vi.mocked(backend.renameFolder).mockResolvedValue("new");
    vi.mocked(backend.listNotes).mockRejectedValue(new Error("列表暂不可用"));
    await renameFolder("old", "new");
    expect(notes.value[0].path).toBe("new/a.md");
    expect(activeNotePath.value).toBe("new/a.md");
  });
  test("相同名称不提交，重复操作被拒绝", async () => {
    await renameNoteFile("old/a.md", "old/a.md");
    expect(backend.renameNoteFile).not.toHaveBeenCalled();
    noteOperationBusy.value = true;
    await expect(renameNoteFile("old/a.md", "old/b.md")).rejects.toThrow("正在处理");
  });
});

describe("后台生成笔记的打开同步", () => {
  test("先补缺失索引与列表，再发布正文；不写回或清空任何笔记", async () => {
    const path = "视频笔记/新笔记.md";
    let finish!: (mtime: number) => void;
    const index = new Promise<number>(resolve => { finish = resolve; });
    vi.mocked(backend.readNote).mockResolvedValue({ path, title: "新笔记", content: "# 完整正文", mtime: 5 });
    vi.mocked(backend.syncNoteIndex).mockReturnValue(index);
    vi.mocked(backend.listNotes).mockResolvedValue([...notes.value, { ...notes.value[0], path, title: "新笔记", mtime: 5 }]);
    const opening = selectNote(path);
    await Promise.resolve();
    expect(backend.syncNoteIndex).toHaveBeenCalledWith(path);
    expect(activeNotePath.value).toBe("old/a.md");
    finish(5);
    await opening;
    expect(notes.value.some(note => note.path === path)).toBe(true);
    expect(activeNotePath.value).toBe(path);
    expect(activeNoteContent.value).toBe("# 完整正文");
    expect(backend.writeNote).not.toHaveBeenCalled();
  });

  test("已收录且未变化的笔记不重建索引", async () => {
    vi.mocked(backend.readNote).mockResolvedValue({ path: "old/a.md", title: "a", content: "原文", mtime: 1 });
    await selectNote("old/a.md");
    expect(backend.syncNoteIndex).not.toHaveBeenCalled();
  });

  test("索引同步期间离开知识库，迟到结果不能恢复旧列表和正文", async () => {
    let finish!: (mtime: number) => void;
    vi.mocked(backend.readNote).mockResolvedValue({ path: "new.md", title: "新笔记", content: "旧知识库正文", mtime: 5 });
    vi.mocked(backend.syncNoteIndex).mockReturnValue(new Promise(resolve => { finish = resolve; }));
    const opening = selectNote("new.md");
    await Promise.resolve();
    resetForVaultLeave();
    finish(5);
    await opening;
    expect(backend.listNotes).not.toHaveBeenCalled();
    expect(activeNotePath.value).toBeNull();
    expect(activeNoteContent.value).toBe("");
  });
});
