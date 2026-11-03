import { describe, expect, test } from "vitest";
import {
  documentExtensionOf,
  documentKindOf,
  documentTitleOf,
  isWordGroupPath,
  stripDocumentExtension,
} from "./documentKind";

describe("文档类型判定", () => {
  /** 判定只看扩展名且大小写不敏感；未知扩展名仍按 Markdown 处理（兼容历史数据）。 */
  test("只有 .qword 是单词组，其它扩展名按 Markdown", () => {
    expect(documentKindOf("英语/核心词.qword")).toBe("wordGroup");
    expect(documentKindOf("英语/核心词.QWORD")).toBe("wordGroup");
    expect(documentKindOf("英语/笔记.md")).toBe("markdown");
    expect(documentKindOf("英语/无后缀")).toBe("markdown");
    expect(documentKindOf("a/b.txt")).toBe("markdown");
    expect(isWordGroupPath("a/b.qword")).toBe(true);
    expect(isWordGroupPath("a/b.md")).toBe(false);
  });

  /** 剥离只处理已知扩展名，未知扩展名原样保留，避免误改用户文件名。 */
  test("标题与扩展名剥离只处理已知扩展名", () => {
    expect(stripDocumentExtension("英语/核心词.qword")).toBe("英语/核心词");
    expect(stripDocumentExtension("英语/笔记.MD")).toBe("英语/笔记");
    expect(stripDocumentExtension("英语/数据.txt")).toBe("英语/数据.txt");
    expect(documentExtensionOf("a/b.QWORD")).toBe(".qword");
    expect(documentExtensionOf("a/b.md")).toBe(".md");
    expect(documentExtensionOf("a/b")).toBe("");
    expect(documentExtensionOf("a/b.txt")).toBe("");
    expect(documentTitleOf("目录/核心词.qword")).toBe("核心词");
    expect(documentTitleOf("目录/深层/笔记.md")).toBe("笔记");
  });
});
