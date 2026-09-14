import { describe, expect, it } from "vitest";
import { extractLink, looksLikeBilibili } from "./videoLink";

/**
 * 轻量识别只服务输入提示；真正的解析与合法性由后端负责，
 * 因此这里覆盖常见的链接形态与误判场景即可。
 */
describe("videoLink", () => {
  it("识别完整链接、分享文本与裸号", () => {
    expect(looksLikeBilibili("https://www.bilibili.com/video/BV1Qwby6DEu1")).toBe(true);
    expect(looksLikeBilibili("【视频】https://b23.tv/abc123 分享自 B 站")).toBe(true);
    expect(looksLikeBilibili("BV1Qwby6DEu1")).toBe(true);
    expect(looksLikeBilibili("av12345")).toBe(true);
  });

  it("不把其他站点或普通文本当作 B 站链接", () => {
    expect(looksLikeBilibili("https://www.youtube.com/watch?v=abc")).toBe(false);
    expect(looksLikeBilibili("save123")).toBe(false);
    expect(looksLikeBilibili("")).toBe(false);
  });

  it("摘出第一个链接并去掉尾部标点", () => {
    expect(extractLink("看这个 https://b23.tv/abc123。很好")).toBe("https://b23.tv/abc123");
    expect(extractLink("BV1Qwby6DEu1")).toBe("BV1Qwby6DEu1");
    expect(extractLink("随便一段话")).toBe("");
  });
});
