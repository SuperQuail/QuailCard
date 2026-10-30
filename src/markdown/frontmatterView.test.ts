import { markdown, markdownLanguage } from "@codemirror/lang-markdown";
import { EditorState } from "@codemirror/state";
import { EditorView } from "@codemirror/view";
import { describe, expect, test } from "vitest";
import { readingMode } from "../editor/readingMode";
import { frontmatterMarkdown } from "./frontmatter";
import { frontmatterExtension, parseFrontmatterEntries } from "./frontmatterView";

/** 用正式语法扩展建视图；阅读态由 readingMode facet 控制。 */
function createView(doc: string, reading: boolean): EditorView {
  return new EditorView({
    parent: document.body,
    state: EditorState.create({
      doc,
      extensions: [
        markdown({ base: markdownLanguage, extensions: [frontmatterMarkdown] }),
        frontmatterExtension(),
        ...(reading ? [readingMode.of(true)] : []),
      ],
    }),
  });
}

const doc = "---\ntags: [工作, 学习]\naliases:\n  - 甲\n  - 乙\n备注: 原样值\n---\n正文";

describe("frontmatter 视图扩展", () => {
  test("编辑态保留原文，只给 frontmatter 三行加弱化样式", () => {
    const view = createView(doc, false);
    try {
      expect(view.dom.textContent).toContain("tags: [工作, 学习]");
      expect(view.dom.querySelectorAll(".qc-frontmatter-line")).toHaveLength(7);
      expect(view.dom.querySelector(".qc-frontmatter-props")).toBeNull();
    } finally {
      view.destroy();
    }
  });

  test("阅读态折叠成属性块：tags/aliases 用 chip，其余键值原样", () => {
    const view = createView(doc, true);
    try {
      const panel = view.dom.querySelector(".qc-frontmatter-props");
      expect(panel).toBeTruthy();
      expect(panel!.textContent).toContain("属性");
      expect(panel!.textContent).toContain("工作");
      expect(panel!.textContent).toContain("学习");
      expect(panel!.textContent).toContain("甲");
      expect(panel!.textContent).toContain("乙");
      expect(panel!.textContent).toContain("备注");
      expect(panel!.textContent).toContain("原样值");
      // 原文被折叠，不再出现在阅读视图里；正文仍然保留。
      expect(view.dom.textContent).not.toContain("tags: [工作, 学习]");
      expect(view.dom.textContent).toContain("正文");
      expect(view.dom.querySelectorAll(".qc-frontmatter-line")).toHaveLength(0);
    } finally {
      view.destroy();
    }
  });

  test("空 frontmatter 在阅读态给出空状态而不是空白块", () => {
    const view = createView("---\n---\n正文", true);
    try {
      expect(view.dom.textContent).toContain("（没有可展示的属性）");
      expect(view.dom.textContent).toContain("正文");
    } finally {
      view.destroy();
    }
  });

  test("没有 frontmatter 的文档不产生任何装饰", () => {
    const view = createView("# 标题\n正文", false);
    try {
      expect(view.dom.querySelectorAll(".qc-frontmatter-line")).toHaveLength(0);
      expect(view.dom.textContent).toContain("# 标题");
    } finally {
      view.destroy();
    }
  });
});

describe("frontmatter 属性解析", () => {
  test("tags/aliases 支持内联列表、逗号分隔与块列表", () => {
    const entries = parseFrontmatterEntries("---\ntags: [a, b]\naliases: 甲, 乙\ntag:\n  - c\n---");
    expect(entries).toEqual([
      { key: "tags", text: "[a, b]", chips: ["a", "b"] },
      { key: "aliases", text: "甲, 乙", chips: ["甲", "乙"] },
      { key: "tag", text: "", chips: ["c"] },
    ]);
  });

  test("未知键与认不出的行都原样保留，注释与空行跳过", () => {
    const entries = parseFrontmatterEntries("---\n# 注释\n标题: 卡片\n无法识别的一行\n---");
    expect(entries).toEqual([
      { key: "标题", text: "卡片", chips: null },
      { key: "", text: "无法识别的一行", chips: null },
    ]);
  });
});
