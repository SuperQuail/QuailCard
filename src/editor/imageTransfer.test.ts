import { describe, expect, test } from "vitest";
import { formatImageInsertion } from "./imageTransfer";

describe("图片 Markdown 插入排版", () => {
  test("文首图片与后续标题之间补换行", () => {
    expect(formatImageInsertion("# 标题", 0, 0, "![图](attachments/a.png)"))
      .toBe("![图](attachments/a.png)\n");
  });

  test("行中插入时与两侧正文分行", () => {
    expect(formatImageInsertion("前文后文", 2, 2, "![图](attachments/a.png)"))
      .toBe("\n![图](attachments/a.png)\n");
  });

  test("独立空行位置不重复添加换行", () => {
    expect(formatImageInsertion("前文\n\n后文", 3, 3, "![图](attachments/a.png)"))
      .toBe("![图](attachments/a.png)");
  });
});
