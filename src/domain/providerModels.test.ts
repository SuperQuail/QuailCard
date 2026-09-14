import { describe, expect, test } from "vitest";
import { formatTokenCount, modelIssue, normalizeModels, parseTokenCount } from "./providerModels";

describe("模型目录 token 输入", () => {
  test("支持纯数字与 K/M 后缀，空串表示未设置", () => {
    expect(parseTokenCount("")).toBeNull();
    expect(parseTokenCount("   ")).toBeNull();
    expect(parseTokenCount("131072")).toBe(131072);
    expect(parseTokenCount("32k")).toBe(32768);
    expect(parseTokenCount("1M")).toBe(1048576);
    expect(parseTokenCount("1.5M")).toBe(1572864);
    expect(parseTokenCount("abc")).toBeNull();
    expect(parseTokenCount("-5")).toBeNull();
    expect(parseTokenCount("0")).toBeNull();
  });

  test("能整除时反向格式化为 K/M，否则原样显示", () => {
    expect(formatTokenCount(null)).toBe("");
    expect(formatTokenCount(1048576)).toBe("1M");
    expect(formatTokenCount(131072)).toBe("128K");
    expect(formatTokenCount(3000)).toBe("3000");
  });

  test("整表规范化丢弃空 ID 与重复项，并补默认显示名称", () => {
    expect(
      normalizeModels([
        { id: " a ", name: " ", contextWindow: null, maxOutputTokens: null },
        { id: "a", name: "重复项", contextWindow: 2048, maxOutputTokens: 4096 },
        { id: "", name: "空 ID", contextWindow: null, maxOutputTokens: null },
      ]),
    ).toEqual([{ id: "a", name: "a", contextWindow: null, maxOutputTokens: null }]);
  });

  test("越界数值给出可读提示，合法配置返回 null", () => {
    expect(modelIssue({ id: "", name: "", contextWindow: null, maxOutputTokens: null })).toBe("请填写模型 ID");
    expect(modelIssue({ id: "m", name: "", contextWindow: 512, maxOutputTokens: null })).toContain("上下文窗口");
    expect(modelIssue({ id: "m", name: "", contextWindow: null, maxOutputTokens: 500_000 })).toContain("最大输出 token");
    expect(modelIssue({ id: "m", name: "", contextWindow: 1048576, maxOutputTokens: 32768 })).toBeNull();
  });
});
