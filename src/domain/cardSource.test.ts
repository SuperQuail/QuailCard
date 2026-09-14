import { describe, expect, test } from "vitest";
import { captureCardSource, isCardSourceCurrent } from "./cardSource";

describe("UTF-16 来源上下文", () => {
  test("上下文边缘的 emoji 不被切为单独代理项", () => {
    const document = `😀${"甲".repeat(47)}来源${"乙".repeat(47)}😀`;
    const source = captureCardSource(document, 49, 51);
    expect(source).toMatchObject({ from: 49, to: 51, excerpt: "来源", prefix: "甲".repeat(47), suffix: "乙".repeat(47) });
    expect(isCardSourceCurrent(document, source)).toBe(true);
    expect(JSON.parse(JSON.stringify(source))).toEqual(source);
  });

  test("中文、emoji 选区和重复摘录仍保持原 UTF-16 位置", () => {
    const document = "来源😀\n重复\n重复😀";
    const from = document.lastIndexOf("重复");
    const source = captureCardSource(document, from, document.length);
    expect(source.excerpt).toBe("重复😀");
    expect(source.from).toBe(8);
    expect(isCardSourceCurrent(document, source)).toBe(true);
    expect(isCardSourceCurrent(`改${document}`, source)).toBe(false);
  });
});
