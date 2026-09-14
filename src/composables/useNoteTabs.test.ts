import { afterEach, expect, test, vi } from "vitest";
import { effectScope, nextTick, ref } from "vue";
import type { NoteSummary } from "../domain/types";
import type { NotePathChange } from "../domain/notePaths";
import { useNoteTabs } from "./useNoteTabs";

const scopes: ReturnType<typeof effectScope>[] = [];
/** 每例独立释放观察器，验证无需组件或全量应用状态。 */
afterEach(() => { scopes.splice(0).forEach((scope) => scope.stop()); });

/** 构造完整领域摘要，路径就是测试身份，不访问磁盘。 */
function note(path: string): NoteSummary {
  return { path, title: path.replace(/\.md$/, ""), tagsJson: "[]", cardCount: 0, dueCount: 0, mtime: 0 };
}

/** 显式控制保存或选中完成时机，不依赖真实计时器。 */
function deferred() {
  let resolve!: () => void;
  let reject!: (error: unknown) => void;
  const promise = new Promise<void>((yes, no) => { resolve = yes; reject = no; });
  return { promise, resolve, reject };
}

/** 默认选择仅发布成功激活；测试可以替换成吞错或延迟实现。 */
function setup(initial: string | null = null, available = ["a.md", "b.md", "c.md"]) {
  const notes = ref(available.map(note));
  const activePath = ref(initial);
  const vaultPath = ref<string | null>("vault-a");
  const pathChange = ref<NotePathChange | null>(null);
  const busy = ref(false);
  const select = vi.fn(async (path: string) => { activePath.value = path; });
  const flush = vi.fn(async (_path: string) => {});
  const clearActive = vi.fn(() => { activePath.value = null; });
  const onError = vi.fn();
  const options = { notes, activePath, vaultPath, pathChange, busy, select, flush, clearActive, onError };
  const scope = effectScope();
  scopes.push(scope);
  const state = scope.run(() => useNoteTabs(options))!;
  /** 模拟来自搜索、树或卡片的成功打开，不调用标签私有入口。 */
  async function open(...paths: string[]) {
    for (const path of paths) { activePath.value = path; await nextTick(); }
  }
  /** 只读取展示契约，测试不接触内部路径队列。 */
  function paths() { return state.tabs.value.map((tab) => tab.path); }
  return { ...options, ...state, scope, open, paths };
}

// 初始化、同 tick 多次激活与重新打开都应遵循真正的打开顺序。
test("成功激活自动收集、去重、更新标题；未知路径不建标签", async () => {
  const s = setup("b.md");
  expect(s.paths()).toEqual(["b.md"]);
  s.activePath.value = "a.md"; s.activePath.value = "c.md"; s.activePath.value = "a.md";
  await nextTick();
  expect(s.paths()).toEqual(["b.md", "a.md", "c.md"]);
  s.notes.value[0]!.title = "新标题";
  expect(s.tabs.value[1]).toEqual({ path: "a.md", title: "新标题" });
  await s.open("missing.md");
  expect(s.paths()).toEqual(["b.md", "a.md", "c.md"]);
  expect(s.select).not.toHaveBeenCalled();
});

// 关闭只改变标签，保留原始文件列表，重新打开应排在末尾。
test("关闭后台标签先保存但不换焦点，重开追加到末尾", async () => {
  const s = setup(); await s.open("a.md", "b.md", "c.md");
  await s.close("b.md");
  expect(s.flush).toHaveBeenCalledWith("b.md");
  expect(s.select).not.toHaveBeenCalled();
  expect(s.paths()).toEqual(["a.md", "c.md"]);
  expect(s.notes.value).toHaveLength(3);
  await s.open("b.md");
  expect(s.paths()).toEqual(["a.md", "c.md", "b.md"]);
});

// 活动标签优先右邻，再左邻，最后一个才清空编辑区。
test("关闭当前标签按右邻、左邻、清空顺序回退", async () => {
  const s = setup(); await s.open("a.md", "b.md", "c.md", "b.md");
  await s.close("b.md");
  expect(s.activePath.value).toBe("c.md");
  expect(s.paths()).toEqual(["a.md", "c.md"]);
  await s.close("c.md"); expect(s.activePath.value).toBe("a.md");
  await s.close("a.md");
  expect(s.clearActive).toHaveBeenCalledOnce();
  expect(s.activePath.value).toBeNull(); expect(s.paths()).toEqual([]);
  expect(s.notes.value).toHaveLength(3);
});

// 原始异常可能包含凭据或路径，错误通知只能使用固定安全文案。
test("保存失败保留标签与焦点，释放关闭锁且不泄露异常", async () => {
  const s = setup("a.md");
  s.flush.mockRejectedValueOnce(new Error("secret-token /private/vault"));
  await s.close("a.md");
  expect(s.paths()).toEqual(["a.md"]); expect(s.activePath.value).toBe("a.md");
  expect(s.onError).toHaveBeenCalledWith("笔记保存失败，标签未关闭，请重试。");
  expect(s.select).not.toHaveBeenCalled(); expect(s.clearActive).not.toHaveBeenCalled();
  expect(s.closing.value).toBe(false);
  await s.close("a.md"); expect(s.paths()).toEqual([]);
});

// store 的 select 会捕获异常，因此 resolved Promise 不等于成功打开。
test.each(["throw", "swallow"])("选择失败（%s）不能移除当前标签", async (mode) => {
  const s = setup(); await s.open("a.md", "b.md", "a.md");
  s.select.mockImplementationOnce(async () => { if (mode === "throw") throw new Error("secret"); });
  await s.close("a.md");
  expect(s.paths()).toEqual(["a.md", "b.md"]); expect(s.activePath.value).toBe("a.md");
  expect(s.onError).toHaveBeenCalledWith("无法切换笔记，标签未关闭，请重试。");
  expect(s.closing.value).toBe(false);
});

// 未完成保存前不允许相邻选择，也不允许并行关闭另一标签。
test("busy、重复关闭和未知标签拒绝新操作；保存完成才选择", async () => {
  const s = setup(); await s.open("a.md", "b.md", "a.md");
  s.busy.value = true; await s.close("a.md");
  s.busy.value = false; await s.close("missing.md");
  expect(s.flush).not.toHaveBeenCalled();
  const gate = deferred(); s.flush.mockReturnValueOnce(gate.promise);
  const task = s.close("a.md"); await nextTick();
  expect(s.closing.value).toBe(true); expect(s.select).not.toHaveBeenCalled();
  await s.close("b.md"); await s.close("a.md");
  expect(s.flush).toHaveBeenCalledTimes(1);
  gate.resolve(); await task;
  expect(s.select).toHaveBeenCalledWith("b.md"); expect(s.paths()).toEqual(["b.md"]);
});

// 等待保存期间用户自主选择优先于关闭时的旧焦点快照。
test.each(["leave", "return", "activate-background"])("等待保存不抢焦点：%s", async (mode) => {
  const s = setup(); await s.open("a.md", "b.md", mode === "activate-background" ? "b.md" : "a.md");
  const gate = deferred(); s.flush.mockReturnValueOnce(gate.promise);
  const task = s.close("a.md"); await nextTick();
  await s.open("b.md");
  if (mode !== "leave") await s.open("a.md");
  gate.resolve(); await task;
  expect(s.select).not.toHaveBeenCalled(); expect(s.clearActive).not.toHaveBeenCalled();
  expect(s.paths()).toEqual(mode === "leave" ? ["b.md"] : ["a.md", "b.md"]);
});

// 与 noteStore 一致按 notes → activePath → pathChange 发布，目录迁移不得误伤同前缀目录。
test.each([["old/a.md", "new.md"], ["old", "old/nested"]])("改名 %s → %s 保持原位置并去重", async (oldPath, newPath) => {
  const s = setup(null, ["old/a.md", "old/b.md", "older/c.md"]);
  await s.open("old/a.md", "older/c.md", "old/b.md", "old/a.md");
  // 独立构造 store 发布的最终摘要，以验证迁移与目录边界。
  const map = (path: string) => path === oldPath || path.startsWith(`${oldPath}/`) ? newPath + path.slice(oldPath.length) : path;
  s.notes.value = s.notes.value.map((entry) => note(map(entry.path)));
  s.activePath.value = map("old/a.md");
  s.pathChange.value = { oldPath, newPath };
  await nextTick();
  expect(s.paths()).toEqual([map("old/a.md"), "older/c.md", map("old/b.md")]);
  expect(s.tabs.value[0]!.title).toBe(map("old/a.md").replace(/\.md$/, ""));
  s.notes.value = s.notes.value.filter((entry) => entry.path !== map("old/a.md"));
  await nextTick(); expect(s.paths()).toEqual(["older/c.md", map("old/b.md")]);
  expect(s.select).not.toHaveBeenCalled();
});

// 换库先发布 vaultPath 的真实顺序，旧激活不能自动继承到新库同名文件。
test("vault立即清空、隔离同名路径，重新成功激活才添加", async () => {
  const s = setup(); await s.open("a.md", "b.md");
  s.vaultPath.value = "vault-b";
  expect(s.paths()).toEqual([]);
  s.notes.value = [note("a.md"), note("b.md")]; await nextTick();
  expect(s.paths()).toEqual([]);
  s.activePath.value = null; await s.open("b.md");
  expect(s.paths()).toEqual(["b.md"]);
  s.vaultPath.value = null; await nextTick(); expect(s.paths()).toEqual([]);
  await s.close("b.md"); expect(s.flush).not.toHaveBeenCalled();
});

// 在任一 await 边界变化身份都要取消旧关闭，甚至换库后又切回也一样。
test.each(["flush", "select"])("%s等待中改名、删除、换库或卸载均隔离旧结果", async (phase) => {
  for (const mutation of ["rename", "delete", "vault", "dispose"]) {
    const s = setup(); await s.open("a.md", "b.md", "a.md");
    const gate = deferred();
    if (phase === "flush") s.flush.mockReturnValueOnce(gate.promise);
    else s.select.mockReturnValueOnce(gate.promise);
    const task = s.close("a.md");
    await nextTick(); await nextTick(); await nextTick();
    if (phase === "select") expect(s.select).toHaveBeenCalledWith("b.md");
    if (mutation === "rename") {
      s.notes.value[0] = note("new.md"); s.activePath.value = "new.md";
      s.pathChange.value = { oldPath: "a.md", newPath: "new.md" };
    } else if (mutation === "delete") {
      s.notes.value = [note("b.md")]; s.activePath.value = "b.md";
    } else if (mutation === "vault") {
      s.vaultPath.value = "vault-b"; s.vaultPath.value = "vault-a";
      s.activePath.value = null; await s.open("a.md");
    } else s.scope.stop();
    await nextTick(); const before = s.paths();
    gate.resolve(); await task;
    expect(s.paths()).toEqual(before); expect(s.onError).not.toHaveBeenCalled();
    expect(s.clearActive).not.toHaveBeenCalled(); expect(s.closing.value).toBe(false);
    if (phase === "flush") expect(s.select).not.toHaveBeenCalled();
  }
});

// select 等待中若另一个成功激活获胜，原标签必须保留而非相信 Promise。
test("等待相邻选择期间用户打开别处，不移除原标签", async () => {
  const s = setup(); await s.open("a.md", "b.md", "c.md", "a.md");
  const gate = deferred(); s.select.mockReturnValueOnce(gate.promise);
  const task = s.close("a.md"); await nextTick(); await nextTick(); await nextTick();
  await s.open("c.md"); gate.resolve(); await task;
  expect(s.paths()).toEqual(["a.md", "b.md", "c.md"]); expect(s.activePath.value).toBe("c.md");
  expect(s.onError).not.toHaveBeenCalled();
});

// 选择真正落地之前保留当前标签和关闭锁，成功后才提交移除。
test("延迟相邻选择成功后才移除当前标签", async () => {
  const s = setup(); await s.open("a.md", "b.md", "a.md");
  const gate = deferred();
  s.select.mockImplementationOnce(async (path) => { await gate.promise; s.activePath.value = path; });
  const task = s.close("a.md"); await nextTick(); await nextTick(); await nextTick();
  expect(s.paths()).toEqual(["a.md", "b.md"]); expect(s.closing.value).toBe(true);
  gate.resolve(); await task;
  expect(s.paths()).toEqual(["b.md"]); expect(s.activePath.value).toBe("b.md");
  expect(s.closing.value).toBe(false);
});

// 后台失败也有身份归属，不能把旧库错误通知给正在使用新库的用户。
test("换库后的迟到保存失败不污染新库标签或错误", async () => {
  const s = setup("a.md"); const gate = deferred();
  s.flush.mockReturnValueOnce(gate.promise);
  const task = s.close("a.md"); await nextTick();
  s.vaultPath.value = "vault-b"; s.activePath.value = null; await s.open("a.md");
  gate.reject(new Error("secret")); await task;
  expect(s.paths()).toEqual(["a.md"]); expect(s.onError).not.toHaveBeenCalled();
  expect(s.clearActive).not.toHaveBeenCalled();
});

// 同一 tick 多次路径发布不能合并丢失中间迁移，也不能改写旧位置。
test("连续两次改名仍保持初次打开顺序", async () => {
  const s = setup(); await s.open("a.md", "b.md", "a.md");
  s.notes.value[0] = note("x.md"); s.activePath.value = "x.md";
  s.pathChange.value = { oldPath: "a.md", newPath: "x.md" };
  s.notes.value[0] = note("y.md"); s.activePath.value = "y.md";
  s.pathChange.value = { oldPath: "x.md", newPath: "y.md" };
  await nextTick(); expect(s.paths()).toEqual(["y.md", "b.md"]);
});

// 保存更新摘要对象及 mtime 不改变笔记身份，不能误判为删除或改名。
test("保存更新元数据不阻止关闭；保存期间进入busy不继续选择", async () => {
  const s = setup(); await s.open("a.md", "b.md", "a.md");
  s.flush.mockImplementationOnce(async () => { s.notes.value = s.notes.value.map((entry) => ({ ...entry, mtime: 1 })); });
  await s.close("a.md"); expect(s.paths()).toEqual(["b.md"]);
  const gate = deferred(); s.flush.mockReturnValueOnce(gate.promise);
  const task = s.close("b.md"); await nextTick(); s.busy.value = true;
  gate.resolve(); await task;
  expect(s.paths()).toEqual(["b.md"]); expect(s.clearActive).not.toHaveBeenCalled();
});
