import { afterEach, describe, expect, test, vi } from "vitest";
import { createNotePersistence } from "./notePersistence";

afterEach(() => vi.useRealTimers());

describe("笔记保存顺序与失败恢复", () => {
  test("旧读取在写盘成功后返回也不能覆盖新内容", async () => {
    const persistence = createNotePersistence(vi.fn().mockResolvedValue(2), vi.fn());
    persistence.register("a.md", "旧内容");
    const readRevision = persistence.states.get("a.md")!.revision;
    persistence.update("a.md", "新内容");
    await persistence.flush("a.md");
    expect(persistence.register("a.md", "旧内容", readRevision)).toBe("新内容");
  });
  test("防抖不丢失切换前的草稿，保留 CRLF 与代码语言", async () => {
    vi.useFakeTimers();
    const write = vi.fn().mockResolvedValue(2);
    const persistence = createNotePersistence(write, vi.fn());
    persistence.register("a.md", "```ts\r\nconst x = 1;\r\n```\r\n");
    persistence.update("a.md", "```ts\nconst x = 2;\n```\n");
    persistence.register("b.md", "另一篇");
    await vi.advanceTimersByTimeAsync(600);
    expect(write).toHaveBeenCalledWith("a.md", "```ts\r\nconst x = 2;\r\n```\r\n");
    expect(persistence.states.get("a.md")?.status).toBe("saved");
  });

  test("旧写入完成后继续写最新版本，最大并发为一", async () => {
    let release!: (mtime: number) => void;
    const write = vi.fn().mockImplementationOnce(() => new Promise<number>((resolve) => { release = resolve; })).mockResolvedValue(3);
    const persistence = createNotePersistence(write, vi.fn());
    persistence.register("a.md", "0");
    persistence.update("a.md", "1");
    const first = persistence.flush("a.md");
    await Promise.resolve(); await Promise.resolve();
    persistence.update("a.md", "2");
    const second = persistence.flush("a.md");
    expect(write).toHaveBeenCalledTimes(1);
    release(2);
    await Promise.all([first, second]);
    expect(write.mock.calls).toEqual([["a.md", "1"], ["a.md", "2"]]);
    expect(persistence.states.get("a.md")?.savedContent).toBe("2");
  });

  test("失败保留草稿与错误，磁盘重读不覆盖，显式重试成功", async () => {
    const write = vi.fn().mockRejectedValueOnce(new Error("磁盘写入失败")).mockResolvedValue(3);
    const persistence = createNotePersistence(write, vi.fn());
    persistence.register("a.md", "旧内容");
    persistence.update("a.md", "新内容");
    await expect(persistence.flushAll()).rejects.toThrow("磁盘写入失败");
    expect(persistence.register("a.md", "旧内容")).toBe("新内容");
    expect(persistence.states.get("a.md")?.status).toBe("error");
    await persistence.flush("a.md");
    expect(persistence.states.get("a.md")?.status).toBe("saved");
  });

  test("改名前保存，改名后没有旧路径定时器", async () => {
    vi.useFakeTimers();
    const write = vi.fn().mockResolvedValue(2);
    const persistence = createNotePersistence(write, vi.fn());
    persistence.register("old/a.md", "0");
    persistence.update("old/a.md", "1");
    await persistence.flushAll();
    persistence.rename("old", "new");
    persistence.update("new/a.md", "2");
    await vi.advanceTimersByTimeAsync(1000);
    expect(write.mock.calls).toEqual([["old/a.md", "1"], ["new/a.md", "2"]]);
    expect(persistence.states.has("old/a.md")).toBe(false);
  });
});
