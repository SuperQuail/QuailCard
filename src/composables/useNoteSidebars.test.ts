import { beforeEach, describe, expect, test, vi } from "vitest";
import { ref } from "vue";
import { useNoteSidebars } from "./useNoteSidebars";
import { notes, activeNotePath } from "../services/stores/noteStore";
import type { NoteSummary } from "../domain/types";

/** 构造测试用笔记摘要。 */
function note(path: string, title: string, aliases: string[] = []): NoteSummary {
  return { path, title, aliases, tagsJson: "[]", cardCount: 0, dueCount: 0, mtime: 1 };
}

/** 记录编辑器收到的定位指令。 */
function editorSpy() {
  const calls: string[] = [];
  return { calls, editor: ref({ revealPosition: (pos: number) => calls.push("pos:" + pos), revealLine: (line: number) => calls.push("line:" + line) }) };
}

beforeEach(() => {
  notes.value = [note("a.md", "a"), note("目录/b.md", "b", ["Bee"])];
  activeNotePath.value = "a.md";
});

describe("大纲与反链跳转", () => {
  test("点击大纲标题滚动到对应偏移", () => {
    const { calls, editor } = editorSpy();
    const { jumpToHeading } = useNoteSidebars({ editor, openNote: vi.fn(), createNote: vi.fn(), askConfirm: vi.fn() });
    jumpToHeading(42);
    expect(calls).toEqual(["pos:42"]);
  });

  test("点开反链先切笔记再定位行", async () => {
    const { calls, editor } = editorSpy();
    const openNote = vi.fn(async () => { calls.push("open"); });
    const { openBacklink } = useNoteSidebars({ editor, openNote, createNote: vi.fn(), askConfirm: vi.fn() });
    await openBacklink("目录/b.md", 7);
    expect(openNote).toHaveBeenCalledWith("目录/b.md");
    expect(calls).toEqual(["open", "line:7"]);
  });
});

describe("打开双链", () => {
  test("已解析时直接跳转，不询问", async () => {
    const { editor } = editorSpy();
    const openNote = vi.fn(async () => {});
    const askConfirm = vi.fn(async () => true);
    const createNote = vi.fn(async () => true);
    const { openLink } = useNoteSidebars({ editor, openNote, createNote, askConfirm });
    await openLink("Bee");
    expect(openNote).toHaveBeenCalledWith("目录/b.md");
    expect(askConfirm).not.toHaveBeenCalled();
    expect(createNote).not.toHaveBeenCalled();
  });

  test("#标题 不参与解析，解析的是笔记部分", async () => {
    const { editor } = editorSpy();
    const openNote = vi.fn(async () => {});
    const { openLink } = useNoteSidebars({ editor, openNote, createNote: vi.fn(), askConfirm: vi.fn() });
    await openLink("目录/b#第二节");
    expect(openNote).toHaveBeenCalledWith("目录/b.md");
  });

  test("未解析时先确认，确认后在同一目录新建", async () => {
    const { editor } = editorSpy();
    const openNote = vi.fn(async () => {});
    const askConfirm = vi.fn(async () => true);
    const createNote = vi.fn(async () => true);
    const { openLink } = useNoteSidebars({ editor, openNote, createNote, askConfirm });
    activeNotePath.value = "目录/b.md";
    await openLink("新想法");
    expect(askConfirm).toHaveBeenCalledTimes(1);
    expect(createNote).toHaveBeenCalledWith("新想法", "目录");
  });

  test("取消确认时什么都不做", async () => {
    const { editor } = editorSpy();
    const createNote = vi.fn(async () => true);
    const { openLink } = useNoteSidebars({ editor, openNote: vi.fn(), createNote, askConfirm: vi.fn(async () => false) });
    await openLink("新想法");
    expect(createNote).not.toHaveBeenCalled();
  });

  test("空目标不触发任何动作", async () => {
    const { editor } = editorSpy();
    const askConfirm = vi.fn(async () => true);
    const { openLink } = useNoteSidebars({ editor, openNote: vi.fn(), createNote: vi.fn(), askConfirm });
    await openLink("   ");
    expect(askConfirm).not.toHaveBeenCalled();
  });
});
