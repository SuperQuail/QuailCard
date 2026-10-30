import { beforeEach, describe, expect, test, vi, type Mock } from "vitest";
import { useNoteActions, type DeleteConfirmRequest } from "./useNoteActions";
import { renameFolder, renameNoteFile } from "../services/stores/noteStore";
import { deleteFolder, deleteNoteFile } from "../services/appState";
import { permanentDelete } from "../services/stores/uiStore";

vi.mock("../services/stores/uiStore", async () => {
  const { ref } = await import("vue");
  return { permanentDelete: ref(false) };
});
vi.mock("../services/appState", () => ({
  selectNote: vi.fn(),
  createNoteFile: vi.fn(),
  deleteFolder: vi.fn(),
  deleteNoteFile: vi.fn(),
}));
vi.mock("../services/stores/noteStore", () => ({ renameFolder: vi.fn(), renameNoteFile: vi.fn() }));

/** 回收站不可用的后端错误：只带 code + 安全 message。 */
const trashUnavailable = { code: "TRASH_UNAVAILABLE", message: "无法移到回收站" };

/** 确认回调 mock：签名与用例注入契约一致，便于检查请求文案。 */
type ConfirmMock = Mock<(request: DeleteConfirmRequest) => Promise<boolean>>;

/** 生成确认回调 mock；默认同意，传 false 表示用户取消。 */
function createConfirm(answer = true): ConfirmMock {
  return vi.fn(async (_request: DeleteConfirmRequest): Promise<boolean> => answer);
}

/** 收集提示与确认的壳层用例实例。 */
function actions(confirm: ConfirmMock = createConfirm()) {
  const toast = vi.fn();
  return { toast, confirm, ...useNoteActions({ showToast: toast, confirm }) };
}

beforeEach(() => {
  vi.resetAllMocks();
  permanentDelete.value = false;
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

describe("批量删除", () => {
  test("永久删除关闭时不弹确认，直接把每项移入回收站", async () => {
    const { toast, confirm, handleDeleteSelection } = actions();
    await handleDeleteSelection([
      { kind: "note", path: "英语/单词.md" },
      { kind: "folder", path: "数学" },
    ]);
    expect(confirm).not.toHaveBeenCalled();
    expect(deleteNoteFile).toHaveBeenCalledWith("英语/单词.md", false);
    expect(deleteFolder).toHaveBeenCalledWith("数学", false);
    expect(toast).toHaveBeenCalledWith("已移到回收站 2 项");
  });

  test("永久删除开启时取消确认，后端零调用", async () => {
    permanentDelete.value = true;
    const { toast, confirm, handleDeleteSelection } = actions(createConfirm(false));
    await handleDeleteSelection([
      { kind: "note", path: "英语/单词.md" },
      { kind: "note", path: "英语/短语.md" },
    ]);
    expect(confirm).toHaveBeenCalledTimes(1);
    const request = confirm.mock.calls[0]![0];
    expect(request.title).toBe("永久删除");
    expect(request.body).toContain("2");
    expect(request.body).toContain("单词");
    expect(deleteNoteFile).not.toHaveBeenCalled();
    expect(deleteFolder).not.toHaveBeenCalled();
    expect(toast).not.toHaveBeenCalled();
  });

  test("永久删除开启时确认后逐项以 permanent=true 调用", async () => {
    permanentDelete.value = true;
    const { toast, handleDeleteSelection } = actions();
    await handleDeleteSelection([
      { kind: "note", path: "甲.md" },
      { kind: "folder", path: "乙" },
    ]);
    expect(deleteNoteFile).toHaveBeenCalledWith("甲.md", true);
    expect(deleteFolder).toHaveBeenCalledWith("乙", true);
    expect(toast).toHaveBeenCalledWith("已永久删除 2 项");
  });

  test("回收站不可用时二次确认，并把当前项及后续项改为永久删除", async () => {
    vi.mocked(deleteNoteFile).mockImplementation(async (_path: string, permanent: boolean) => {
      if (!permanent) throw trashUnavailable;
    });
    const { toast, confirm, handleDeleteSelection } = actions();
    await handleDeleteSelection([
      { kind: "note", path: "甲.md" },
      { kind: "note", path: "乙.md" },
    ]);
    expect(confirm).toHaveBeenCalledTimes(1);
    expect(confirm.mock.calls[0]![0].body).toContain("无法移到回收站");
    expect(deleteNoteFile).toHaveBeenNthCalledWith(1, "甲.md", false);
    expect(deleteNoteFile).toHaveBeenNthCalledWith(2, "甲.md", true);
    expect(deleteNoteFile).toHaveBeenNthCalledWith(3, "乙.md", true);
    expect(toast).toHaveBeenCalledWith("已永久删除 2 项");
  });

  test("回收站不可用时取消二次确认，中止并提示已处理条数", async () => {
    vi.mocked(deleteNoteFile).mockImplementation(async (_path: string, permanent: boolean) => {
      if (!permanent) throw trashUnavailable;
    });
    const { toast, handleDeleteSelection } = actions(createConfirm(false));
    await handleDeleteSelection([
      { kind: "note", path: "甲.md" },
      { kind: "note", path: "乙.md" },
    ]);
    expect(deleteNoteFile).toHaveBeenCalledTimes(1);
    expect(toast).toHaveBeenCalledWith("已处理 0 项，其余未删除");
  });

  test("其他错误按原有语义立即停止并提示原因", async () => {
    vi.mocked(deleteNoteFile).mockRejectedValue({ code: "FILE_ERROR", message: "没有权限访问该文件" });
    const { toast, handleDeleteSelection } = actions();
    await handleDeleteSelection([
      { kind: "note", path: "甲.md" },
      { kind: "note", path: "乙.md" },
    ]);
    expect(deleteNoteFile).toHaveBeenCalledTimes(1);
    expect(toast).toHaveBeenCalledWith("没有权限访问该文件");
  });
});
