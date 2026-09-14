import { expect, test } from "vitest";
import { clipboardImages, readAgentImages } from "./agentImages";

test("读取剪贴板图片为模型附件，普通文字不被当作图片", async () => {
  const file = new File(["image"], "paste.png", { type: "image/png" });
  const files = clipboardImages({ files: [file], items: [] } as unknown as DataTransfer);
  expect(await readAgentImages(files, [])).toEqual([{ name: "paste.png", mimeType: "image/png", dataBase64: "aW1hZ2U=" }]);
  expect(clipboardImages(null)).toEqual([]);
  expect(clipboardImages({ files: [], items: [{ kind: "string", type: "text/plain" }] } as unknown as DataTransfer)).toEqual([]);
});

test("拒绝不支持格式、超量与过大附件", async () => {
  const file = new File(["image"], "paste.png", { type: "image/png" });
  await expect(readAgentImages(Array(5).fill(file), [])).rejects.toThrow("4 张");
  await expect(readAgentImages([new File(["gif"], "a.gif", { type: "image/gif" })], [])).rejects.toThrow("仅支持");
  Object.defineProperty(file, "size", { value: 6 * 1024 * 1024 });
  await expect(readAgentImages([file], [])).rejects.toThrow("5 MiB");
});
