import { beforeEach, describe, expect, test, vi } from "vitest";
import { useNoteActions } from "./useNoteActions";
import { renameFolder, renameNoteFile } from "../services/stores/noteStore";

vi.mock("../services/appState", () => ({
  selectNote: vi.fn(),
  createNoteFile: vi.fn(),
  deleteFolder: vi.fn(),
  deleteNoteFile: vi.fn(),
}));
vi.mock("../services/stores/noteStore", () => ({ renameFolder: vi.fn(), renameNoteFile: vi.fn() }));

/** 收集提示的壳层用例实例。 */
function actions() {
  const toast = vi.fn();
  return { toast, ...useNoteActions({ showToast: toast }) };
}

beforeEach(() => {
  vi.resetAllMocks();
});

describe("批量移动", () => {
  test("按计划顺序逐项移动并提示总数", async () => {
    const order: string[] = [];
    vi.mocked(renameNoteFile).mockImplementation(async (from: string) => { order.push(from); });
    vi.mocked(renameFolder).mockImplementation(async (from: string) => { order.push(from); });
    const { toast, handleMoveEntries } = actions();
    await handleMoveEntries([
      { kind: "folder", from: "英语", to: "课程/英语" },
      { kind: "note", from: "数学/线性代数.md", to: "课程/线性代数.md" },
    ]);
    expect(order).toEqual(["英语", "数学/线性代数.md"]);
    expect(renameFolder).toHaveBeenCalledWith("英语", "课程/英语");
    expect(renameNoteFile).toHaveBeenCalledWith("数学/线性代数.md", "课程/线性代数.md");
    expect(toast).toHaveBeenCalledWith("已移动 2 项");
  });

  test("中途失败保留已移动结果并说明剩余项", async () => {
    vi.mocked(renameNoteFile).mockImplementation(async (from: string) => {
      if (from === "乙.md") throw new Error("同名笔记已存在");
    });
    const { toast, handleMoveEntries } = actions();
    await handleMoveEntries([
      { kind: "note", from: "甲.md", to: "课程/甲.md" },
      { kind: "note", from: "乙.md", to: "课程/乙.md" },
      { kind: "note", from: "丙.md", to: "课程/丙.md" },
    ]);
    expect(renameNoteFile).toHaveBeenCalledTimes(2);
    expect(toast).toHaveBeenCalledWith("已移动 1 项，其余失败：同名笔记已存在");
  });

  test("空计划不触发任何请求与提示", async () => {
    const { toast, handleMoveEntries } = actions();
    await handleMoveEntries([]);
    expect(renameNoteFile).not.toHaveBeenCalled();
    expect(toast).not.toHaveBeenCalled();
  });
});
