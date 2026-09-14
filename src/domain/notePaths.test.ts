import { describe, expect, test } from "vitest";
import { canMoveInto, parentPath, planNoteMoves, remapNotePath, type NoteMoveItem } from "./notePaths";

/** 构造移动条目，测试只关心路径与类型。 */
function item(kind: "folder" | "note", path: string): NoteMoveItem {
  return { kind, path };
}

describe("parentPath", () => {
  test("根级条目返回空目录，嵌套条目返回完整父路径", () => {
    expect(parentPath("笔记.md")).toBe("");
    expect(parentPath("英语/单词.md")).toBe("英语");
    expect(parentPath("课程/数据结构/线性表.md")).toBe("课程/数据结构");
  });
});

describe("canMoveInto", () => {
  test("原地不动不算移动", () => {
    expect(canMoveInto(item("note", "英语/单词.md"), "英语")).toBe(false);
    expect(canMoveInto(item("folder", "英语"), "")).toBe(false);
  });

  test("文件夹不能移进自身或自身子目录", () => {
    expect(canMoveInto(item("folder", "英语"), "英语")).toBe(false);
    expect(canMoveInto(item("folder", "英语"), "英语/单词")).toBe(false);
    expect(canMoveInto(item("folder", "英语"), "英语角")).toBe(true);
  });

  test("同级前缀目录不受路径边界影响", () => {
    expect(canMoveInto(item("folder", "old"), "older")).toBe(true);
    expect(canMoveInto(item("note", "older/a.md"), "old")).toBe(true);
  });
});

describe("planNoteMoves", () => {
  test("笔记移入目标目录保留文件名", () => {
    expect(planNoteMoves([item("note", "单词.md")], "英语")).toEqual([
      { kind: "note", from: "单词.md", to: "英语/单词.md" },
    ]);
  });

  test("移回根目录只保留文件名", () => {
    expect(planNoteMoves([item("note", "英语/单词.md")], "")).toEqual([
      { kind: "note", from: "英语/单词.md", to: "单词.md" },
    ]);
    expect(planNoteMoves([item("folder", "英语/单词")], "")).toEqual([
      { kind: "folder", from: "英语/单词", to: "单词" },
    ]);
  });

  test("无效落点与原地拖动不产生任何移动", () => {
    expect(planNoteMoves([item("note", "英语/单词.md")], "英语")).toEqual([]);
    expect(planNoteMoves([item("folder", "英语")], "英语/子目录")).toEqual([]);
    expect(planNoteMoves([], "英语")).toEqual([]);
  });

  test("选中父文件夹时丢弃其后代，避免旧路径失效", () => {
    const moves = planNoteMoves([
      item("folder", "英语"),
      item("note", "英语/单词.md"),
      item("note", "数学/线性代数.md"),
    ], "课程");
    expect(moves).toEqual([
      { kind: "folder", from: "英语", to: "课程/英语" },
      { kind: "note", from: "数学/线性代数.md", to: "课程/线性代数.md" },
    ]);
  });
});

describe("remapNotePath", () => {
  test("只按路径边界迁移，不误伤同前缀目录", () => {
    expect(remapNotePath("old/a.md", "old", "new")).toBe("new/a.md");
    expect(remapNotePath("older/a.md", "old", "new")).toBe("older/a.md");
  });
});
