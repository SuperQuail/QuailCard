import { expect, test } from "vitest";
import { formatBytes } from "./video";

// 下载速度和小文件不得四舍五入成 0MB，异常数值不显示伪造进度。
test("formats small download amounts and rejects invalid values", () => {
  expect(formatBytes(512)).toBe("512B");
  expect(formatBytes(512 * 1024)).toBe("512KB");
  expect(formatBytes(1024 * 1024)).toBe("1MB");
  expect(formatBytes(1024 ** 3)).toBe("1.0GB");
  for (const value of [0, -1, Number.NaN, Number.POSITIVE_INFINITY]) expect(formatBytes(value)).toBe("—");
});
