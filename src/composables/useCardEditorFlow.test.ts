import { beforeEach, describe, expect, test, vi } from "vitest";
import { captureCardSource } from "../domain/cardSource";
import { activeNoteContent, activeNotePath, notePersistence } from "../services/stores/noteStore";
import { activeNoteCards } from "../services/stores/cardStore";
import { useCardEditorFlow } from "./useCardEditorFlow";
import * as backend from "../api/backend";

vi.mock("../api/backend", () => ({ saveCard: vi.fn(), writeNote: vi.fn(), getStudyStats: vi.fn().mockResolvedValue({}), resolveErrorMessage: String }));

beforeEach(() => {
  vi.clearAllMocks();
  activeNotePath.value = "笔记.md";
  activeNoteCards.value = [];
  notePersistence.remove();
  vi.mocked(backend.saveCard).mockImplementation(async (input) => ({ ...input, id: "card", sourceRef: "", aliases: [], rubricPoints: [], detail: "", example: "", position: 0, version: 0, schedulerPhase: "new", intervalDays: 0, totalReviews: 0, dueAt: 0 }));
});

describe("划词拆卡", () => {
  test("重新选择来源保留用户已经填写的问答", () => {
    const flow = useCardEditorFlow({ showToast: vi.fn() });
    flow.openCardEditorFromSelection({ notePath: "笔记.md", source: captureCardSource("旧来源", 0, 3) });
    flow.reselectCardSource({ kind: "qa", front: "已填写的问题", back: "整理后的答案", detail: "", example: "", rubric: "" });
    flow.openCardEditorFromSelection({ notePath: "笔记.md", source: captureCardSource("新来源", 0, 3) });
    expect(flow.cardEditor.value).toMatchObject({ open: true, front: "已填写的问题", back: "整理后的答案", source: { excerpt: "新来源" } });
  });
  test.each(["中文答案", "English answer", "中文 English"])("%s 始终填入答案", (text) => {
    const flow = useCardEditorFlow({ showToast: vi.fn() });
    flow.openCardEditorFromSelection({ notePath: "笔记.md", source: captureCardSource(text, 0, text.length) });
    expect(flow.cardEditor.value.front).toBe("");
    expect(flow.cardEditor.value.back).toBe(text);
  });

  test("重复文本定位第二处，保存不重写 Markdown 或注入代码标记", async () => {
    const content = "相同答案\n\n```typescript\nconst answer = '相同答案';\n```\n\n* 列表\n";
    const from = content.lastIndexOf("相同答案");
    activeNoteContent.value = content;
    notePersistence.register("笔记.md", content);
    const flow = useCardEditorFlow({ showToast: vi.fn() });
    flow.openCardEditorFromSelection({ notePath: "笔记.md", source: captureCardSource(content, from, from + 4) });
    await flow.handleCardEditorSave({ kind: "qa", front: "问题", back: "相同答案", detail: "", example: "", rubric: "" });
    expect(backend.saveCard).toHaveBeenCalledWith(expect.objectContaining({ source: expect.objectContaining({ from }) }));
    expect(backend.writeNote).not.toHaveBeenCalled();
    expect(activeNoteContent.value).toBe(content);
  });

  test("来源失效拒绝保存并保留草稿", async () => {
    const flow = useCardEditorFlow({ showToast: vi.fn() });
    flow.openCardEditorFromSelection({ notePath: "笔记.md", source: captureCardSource("原文", 0, 2) });
    activeNoteContent.value = "改动后的原文";
    await flow.handleCardEditorSave({ kind: "qa", front: "我的问题", back: "原文", detail: "", example: "", rubric: "" });
    expect(backend.saveCard).not.toHaveBeenCalled();
    expect(flow.cardEditor.value.open).toBe(true);
    expect(flow.cardEditor.value.front).toBe("我的问题");
  });
});
