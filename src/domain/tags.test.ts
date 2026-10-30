import { describe, expect, test } from "vitest";
import { buildTagTree, filterNotesByTag, normalizeTag, tagsOf } from "./tags";
import type { NoteSummary } from "./types";

/** 构造笔记摘要；只填标签树需要的字段，其余给稳定默认值。 */
function note(path: string, tagsJson: string, title = path): NoteSummary {
  return { path, title, tagsJson, aliases: [], cardCount: 0, dueCount: 0, mtime: 0 };
}

describe("tagsOf", () => {
  test("解析合法数组、归一化写法并去重", () => {
    expect(tagsOf(note("a.md", JSON.stringify(["#工作/项目", "工作/项目/", " 工作/项目 ", "生活"]))))
      .toEqual(["工作/项目", "生活"]);
  });

  test("非法 JSON、非数组与非字符串项都安全返回", () => {
    expect(tagsOf(note("a.md", "{bad"))).toEqual([]);
    expect(tagsOf(note("a.md", JSON.stringify({ tags: ["a"] })))).toEqual([]);
    expect(tagsOf(note("a.md", JSON.stringify([42, null, "a"])))).toEqual(["a"]);
  });

  test("normalizeTag 处理前缀 # 与空段", () => {
    expect(normalizeTag("##a//b/")).toBe("a/b");
    expect(normalizeTag(42)).toBe("");
  });
});

describe("buildTagTree", () => {
  const notes = [
    note("a.md", JSON.stringify(["工作/项目/子"]), "A"),
    note("b.md", JSON.stringify(["工作/项目"]), "B"),
    note("c.md", JSON.stringify(["工作"]), "C"),
    note("d.md", JSON.stringify(["生活"]), "D"),
    note("e.md", "坏数据", "E"),
  ];

  test("a/b/c 展开成树，父计数是子树去重后的笔记数", () => {
    const tree = buildTagTree(notes);
    expect(tree.map((node) => node.fullPath)).toEqual(["工作", "生活"]);
    const work = tree[0]!;
    expect(work.name).toBe("工作");
    expect(work.count).toBe(3);
    const project = work.children[0]!;
    expect(project.fullPath).toBe("工作/项目");
    expect(project.count).toBe(2);
    expect(project.children[0]!.fullPath).toBe("工作/项目/子");
    expect(project.children[0]!.count).toBe(1);
    expect(tree[1]!.count).toBe(1);
  });

  test("同一篇笔记的重复标签不会重复计数", () => {
    const tree = buildTagTree([note("a.md", JSON.stringify(["a", "a/b", "a/b"]))]);
    expect(tree[0]!.count).toBe(1);
    expect(tree[0]!.children[0]!.count).toBe(1);
  });

  test("空输入返回空树", () => {
    expect(buildTagTree([])).toEqual([]);
  });
});

describe("filterNotesByTag", () => {
  const notes = [
    note("a.md", JSON.stringify(["工作/项目"])),
    note("b.md", JSON.stringify(["工作"]), "B"),
    note("c.md", JSON.stringify(["生活"]), "C"),
    note("bad.md", "坏数据", "Bad"),
  ];

  test("父标签包含子标签", () => {
    expect(filterNotesByTag(notes, "工作").map((item) => item.path)).toEqual(["a.md", "b.md"]);
    expect(filterNotesByTag(notes, "工作/项目").map((item) => item.path)).toEqual(["a.md"]);
  });

  test("空标签返回全部笔记，坏数据笔记不会匹配", () => {
    expect(filterNotesByTag(notes, "").map((item) => item.path)).toEqual(["a.md", "b.md", "c.md", "bad.md"]);
    expect(filterNotesByTag(notes, "工作").map((item) => item.path)).not.toContain("bad.md");
  });
});
