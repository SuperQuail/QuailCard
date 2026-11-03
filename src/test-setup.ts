/**
 * jsdom 测试环境补丁：几何 API 与 Web Storage 全局。
 *
 * 1) jsdom 不实现 Range 的布局测量，而 CodeMirror 的异步测量回调会调用
 *    Range.getClientRects；缺少实现时会在 requestAnimationFrame 里抛错，
 *    把 worker 的 jsdom 全局状态带崩。这里补一个空实现，让测量路径安静返回。
 * 2) Node 25 起默认启用 Web Storage：没有 --localstorage-file 时全局
 *    localStorage 只是「读取即返回 undefined」的 stub，jsdom 的同名实现被它顶掉，
 *    直接读 localStorage 的 store 会在 import 阶段抛 TypeError。这里补一个
 *    内存实现，只影响测试环境，不改变浏览器与 WebView 里的真实行为。
 */
if (typeof Range !== "undefined") {
  if (!Range.prototype.getClientRects) {
    Range.prototype.getClientRects = function getClientRects(): DOMRectList {
      const rects = [] as unknown as DOMRectList;
      return rects;
    };
  }
  if (!Range.prototype.getBoundingClientRect) {
    Range.prototype.getBoundingClientRect = function getBoundingClientRect(): DOMRect {
      return new DOMRect();
    };
  }
}

/** 内存版 Storage：满足 store 里 getItem / setItem 即可，不模拟配额与事件。 */
function createMemoryStorage(): Storage {
  const items = new Map<string, string>();
  const storage: Storage = {
    get length(): number {
      return items.size;
    },
    clear(): void {
      items.clear();
    },
    getItem(key: string): string | null {
      return items.get(key) ?? null;
    },
    key(index: number): string | null {
      return [...items.keys()][index] ?? null;
    },
    removeItem(key: string): void {
      items.delete(key);
    },
    setItem(key: string, value: string): void {
      items.set(key, String(value));
    },
  };
  return storage;
}

// Node 顶掉了 jsdom 的存储全局时（getItem 缺失）才接管，正常运行时保持原状。
for (const key of ["localStorage", "sessionStorage"] as const) {
  const current = (globalThis as Record<string, unknown>)[key] as Storage | undefined;
  if (typeof current?.getItem === "function") {
    continue;
  }
  Object.defineProperty(globalThis, key, { value: createMemoryStorage(), configurable: true, writable: true });
}
