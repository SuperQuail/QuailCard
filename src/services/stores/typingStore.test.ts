import { beforeEach, describe, expect, test, vi } from "vitest";
import * as backend from "../../api/backend";
import { applyPracticeSettings, practiceSettings, setPracticeSettings } from "./typingStore";

vi.mock("../../api/backend", () => ({ setPracticeSettings: vi.fn() }));

beforeEach(() => {
  vi.clearAllMocks();
  applyPracticeSettings({ repeatsPerWord: 5, wordsPerPage: 3 });
});

describe("打字背诵偏好 store", () => {
  test("保存成功以后端回显为准", async () => {
    vi.mocked(backend.setPracticeSettings).mockResolvedValue({ repeatsPerWord: 8, wordsPerPage: 2 });
    await setPracticeSettings({ repeatsPerWord: 8, wordsPerPage: 2 });
    expect(backend.setPracticeSettings).toHaveBeenCalledWith({ repeatsPerWord: 8, wordsPerPage: 2 });
    expect(practiceSettings.value).toEqual({ repeatsPerWord: 8, wordsPerPage: 2 });
  });

  test("保存失败保留旧值并抛出可读错误", async () => {
    vi.mocked(backend.setPracticeSettings).mockRejectedValue(new Error("参数超出允许范围"));
    await expect(setPracticeSettings({ repeatsPerWord: 99, wordsPerPage: 1 })).rejects.toThrow("参数超出允许范围");
    expect(practiceSettings.value).toEqual({ repeatsPerWord: 5, wordsPerPage: 3 });
  });
});
