import { markdown } from "@codemirror/lang-markdown";
import { EditorState } from "@codemirror/state";
import { describe, expect, test } from "vitest";
import { collectMarkdownImages, isLocalRelativeImageSource } from "./markdownImages";

/** 创建启用 Markdown 语法树的测试状态。 */
function markdownState(doc: string): EditorState {
  return EditorState.create({ doc, extensions: [markdown()] });
}

describe("Markdown 图片预览检测", () => {
  test("仅收集本地相对图片并保留 alt", () => {
    const state = markdownState("![结构图](../attachments/map.webp)\n![远程](https://example.com/a.png)");
    expect(collectMarkdownImages(state)).toEqual([
      { from: 0, to: 31, alt: "结构图", source: "../attachments/map.webp" },
    ]);
  });

  test("忽略普通链接和行内代码中的图片文本", () => {
    const state = markdownState("[附件](attachments/a.png) 和 `![代码](attachments/code.png)`");
    expect(collectMarkdownImages(state)).toEqual([]);
  });

  test("拒绝绝对路径、协议 URL 与 data URL", () => {
    expect(isLocalRelativeImageSource("attachments/a.png")).toBe(true);
    expect(isLocalRelativeImageSource("/attachments/a.png")).toBe(false);
    expect(isLocalRelativeImageSource("C:\\images\\a.png")).toBe(false);
    expect(isLocalRelativeImageSource("data:image/png;base64,AA")).toBe(false);
  });
});
