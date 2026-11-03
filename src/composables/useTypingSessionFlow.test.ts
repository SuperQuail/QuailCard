import { beforeEach, describe, expect, test, vi } from "vitest";
import { useTypingSessionFlow } from "./useTypingSessionFlow";
import { activeNoteCards } from "../services/stores/cardStore";
import { activeNotePath, notes } from "../services/stores/noteStore";
import type { NoteCard, NoteSummary } from "../domain/types";

/** 构造一张单词卡：只覆盖入口判定用到的字段。 */
function vocabularyCard(id: string, overrides: Partial<NoteCard> = {}): NoteCard {
  return {
    id,
    notePath: "英语/核心词.qword",
    sourceRef: "",
    source: null,
    kind: "vocabulary",
    front: "短暂的",
    back: "ephemeral",
    detail: "/ɪˈfemərəl/",
    example: "",
    aliases: [],
    rubricPoints: [],
    position: 0,
    schedulerPhase: "new",
    dueAt: 0,
    intervalDays: 0,
    totalReviews: 0,
    version: 0,
    ...overrides,
  };
}

/** 构造单词组笔记摘要：标题即文件名去掉 .qword。 */
function wordGroupNote(path: string, title: string): NoteSummary {
  return { path, title, aliases: [], tagsJson: "[]", cardCount: 0, dueCount: 0, mtime: 1 };
}

beforeEach(() => {
  notes.value = [wordGroupNote("英语/核心词.qword", "核心词")];
  activeNotePath.value = "英语/核心词.qword";
  activeNoteCards.value = [];
});

describe("打字背诵入口兼容单词组", () => {
  test("单词组还没有已保存的单词时只提示，不打开工作区", () => {
    const showToast = vi.fn();
    const flow = useTypingSessionFlow({ showToast });
    flow.startTypingFromNote();
    expect(showToast).toHaveBeenCalledWith("这个单词组还没有已保存的单词，先填几页吧");
    expect(flow.typingSession.value.open).toBe(false);
  });

  test("从单词组进入时词池来自本组卡片，标题取组名", () => {
    activeNoteCards.value = [vocabularyCard("page-1")];
    const flow = useTypingSessionFlow({ showToast: vi.fn() });
    flow.startTypingFromNote();
    expect(flow.typingSession.value).toMatchObject({ open: true, scope: "note", notePath: "英语/核心词.qword", title: "打字背诵 · 核心词" });
  });

  test("卡片没变时复用同一个会话，保住已练的进度", () => {
    activeNoteCards.value = [vocabularyCard("page-1")];
    const flow = useTypingSessionFlow({ showToast: vi.fn() });
    flow.startTypingFromNote();
    const first = flow.typingSession.value.id;
    flow.typingSession.value.open = false;
    flow.startTypingFromNote();
    expect(flow.typingSession.value.id).toBe(first);
  });

  test("单词组保存后卡片变化，重建会话避免练到旧词池", () => {
    activeNoteCards.value = [vocabularyCard("page-1")];
    const flow = useTypingSessionFlow({ showToast: vi.fn() });
    flow.startTypingFromNote();
    const first = flow.typingSession.value.id;
    flow.typingSession.value.open = false;
    // 用户回到编辑区改了释义，重新保存后卡片内容变了。
    activeNoteCards.value = [vocabularyCard("page-1", { front: "转瞬即逝的" })];
    flow.startTypingFromNote();
    expect(flow.typingSession.value.id).not.toBe(first);
  });

  test("Markdown 笔记没有单词卡时提示拆卡", () => {
    activeNotePath.value = "英语/笔记.md";
    const showToast = vi.fn();
    const flow = useTypingSessionFlow({ showToast });
    flow.startTypingFromNote();
    expect(showToast).toHaveBeenCalledWith("这篇笔记还没有单词卡，先拆几张吧");
  });
});
