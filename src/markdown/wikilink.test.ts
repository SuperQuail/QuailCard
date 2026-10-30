import { describe, expect, test } from "vitest";
import { markdownParser } from "./parser";
import {
  splitWikiTarget,
  wikiLinkDisplayText,
  wikiLinkSpan,
  WIKILINK,
  WIKILINK_LABEL,
  WIKILINK_TARGET,
} from "./wikilink";

/** 收集文档里的双链节点，返回原始文本、目标与显示名。 */
function collect(doc: string) {
  const found: { text: string; target: string | null; label: string | null }[] = [];
  markdownParser.parse(doc).iterate({
    enter(node) {
      if (node.name !== WIKILINK) return;
      found.push({
        text: doc.slice(node.from, node.to),
        target: node.node.getChild(WIKILINK_TARGET)?.name ?? null,
        label: node.node.getChild(WIKILINK_LABEL)?.name ?? null,
      });
    },
  });
  return found;
}

/** 收集节点子节点名字，验证 WikiLinkTarget / WikiLinkLabel 的结构契约。 */
function childNames(doc: string): string[][] {
  const names: string[][] = [];
  markdownParser.parse(doc).iterate({
    enter(node) {
      if (node.name === WIKILINK) names.push([...node.node.getChildren(WIKILINK_TARGET), ...node.node.getChildren(WIKILINK_LABEL)].map((child) => child.name));
    },
  });
  return names;
}

describe("双链行内语法", () => {
  test("[[目标]] 产出 WikiLink + WikiLinkTarget", () => {
    const doc = "前 [[目标]] 后";
    expect(collect(doc)).toEqual([{ text: "[[目标]]", target: WIKILINK_TARGET, label: null }]);
    expect(childNames(doc)).toEqual([[WIKILINK_TARGET]]);
  });

  test("[[目标|显示]]、[[目标#标题]]、[[目标#标题|显示]] 都产出目标与显示名", () => {
    const doc = "[[目标|显示]] 与 [[dir/B#标题]] 与 [[C#小节|别名]]";
    expect(collect(doc)).toEqual([
      { text: "[[目标|显示]]", target: WIKILINK_TARGET, label: WIKILINK_LABEL },
      { text: "[[dir/B#标题]]", target: WIKILINK_TARGET, label: null },
      { text: "[[C#小节|别名]]", target: WIKILINK_TARGET, label: WIKILINK_LABEL },
    ]);
  });

  test("目标与显示名两侧空白被裁掉", () => {
    const spans: ReturnType<typeof wikiLinkSpan>[] = [];
    const doc = "[[  dir/B  |  显示  ]]";
    markdownParser.parse(doc).iterate({
      enter(node) {
        if (node.name === WIKILINK) spans.push(wikiLinkSpan(node.node, doc));
      },
    });
    expect(spans).toEqual([{ from: 0, to: doc.length, target: "dir/B", label: "显示", embed: false }]);
  });

  test("![[...]] 内嵌形式整段属于 WikiLink", () => {
    const doc = "![[图片.png]]";
    const spans: ReturnType<typeof wikiLinkSpan>[] = [];
    markdownParser.parse(doc).iterate({
      enter(node) {
        if (node.name === WIKILINK) spans.push(wikiLinkSpan(node.node, doc));
      },
    });
    expect(spans).toEqual([{ from: 0, to: doc.length, target: "图片.png", label: null, embed: true }]);
  });

  test("未闭合、空目标、跨行与代码内都不识别为双链", () => {
    for (const doc of ["[[未闭合", "[[]]", "[[a\nb]]", "`[[代码]]`", "![[未闭合"]) {
      expect(collect(doc)).toEqual([]);
    }
  });

  test("普通图片与普通链接不会被双链解析器抢走", () => {
    const doc = "![图](a.png) 与 [文字](b.md)";
    let image = 0;
    let link = 0;
    markdownParser.parse(doc).iterate({
      enter(node) {
        if (node.name === "Image") image++;
        if (node.name === "Link") link++;
        expect(node.name).not.toBe(WIKILINK);
      },
    });
    expect(image).toBe(1);
    expect(link).toBe(1);
  });

  test("转义的 \\[[ 不算双链", () => {
    expect(collect("\\[[转义]]")).toEqual([]);
  });
});

describe("双链读取辅助函数", () => {
  test("splitWikiTarget 剥离 #标题，同时保留笔记部分", () => {
    expect(splitWikiTarget("dir/B#小节 一")).toEqual({ note: "dir/B", heading: "小节 一" });
    expect(splitWikiTarget("B")).toEqual({ note: "B", heading: null });
    expect(splitWikiTarget("B#")).toEqual({ note: "B", heading: null });
    expect(splitWikiTarget("  #仅标题  ")).toEqual({ note: "", heading: "仅标题" });
  });

  test("wikiLinkDisplayText 优先显示名，否则显示目标名（不含 #标题）", () => {
    const base = { from: 0, to: 0, embed: false };
    expect(wikiLinkDisplayText({ ...base, target: "dir/B#小节", label: null })).toBe("dir/B");
    expect(wikiLinkDisplayText({ ...base, target: "dir/B#小节", label: "显示" })).toBe("显示");
  });
});
