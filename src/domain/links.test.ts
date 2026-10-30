import { describe, expect, test } from "vitest";
import rawFixture from "./linkResolution.fixture.json";
import { resolveLink, type LinkTarget } from "./links";

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
