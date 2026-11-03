import { describe, expect, test } from "vitest";
import rawFixture from "./linkResolution.fixture.json";
import { linkTargetsOf, resolveLink, type LinkTarget } from "./links";
import type { NoteSummary } from "./types";

/** 共享夹具：后端 storage/links.rs 跑同一份用例，两端规则一旦漂移就会同时失败。 */
interface Fixture {
  notes: LinkTarget[];
  cases: { name: string; from: string; target: string; expect: string | null }[];
}

const fixture = rawFixture as unknown as Fixture;

describe("双链解析契约", () => {
  test.each(fixture.cases)("$name", ({ from, target, expect: expected }) => {
    expect(resolveLink(target, from, fixture.notes)).toBe(expected);
  });

  test("空目标与非字符串输入都安全返回 null", () => {
    expect(resolveLink("   ", "A.md", fixture.notes)).toBeNull();
    expect(resolveLink("", "A.md", fixture.notes)).toBeNull();
  });

  test("没有任何笔记时解析为空", () => {
    expect(resolveLink("任意目标", "A.md", [])).toBeNull();
  });
});

describe("双链候选过滤单词组", () => {
  /** 单词组不参与 [[双链]]：候选过滤后既不能被解析，也不会成为新建目标。 */
  test("linkTargetsOf 只保留 Markdown，解析不到单词组", () => {
    const summaries: NoteSummary[] = [
      { path: "英语/核心词.qword", title: "核心词", aliases: [], tagsJson: "[]", cardCount: 2, dueCount: 0, mtime: 1 },
      { path: "英语/笔记.md", title: "笔记", aliases: [], tagsJson: "[]", cardCount: 0, dueCount: 0, mtime: 1 },
    ];
    const targets = linkTargetsOf(summaries);
    expect(targets.map((note) => note.path)).toEqual(["英语/笔记.md"]);
    expect(resolveLink("核心词", "英语/其它.md", targets)).toBeNull();
    expect(resolveLink("笔记", "英语/其它.md", targets)).toBe("英语/笔记.md");
  });
});
