/**
 * jsdom 测试环境的几何补丁。
 *
 * jsdom 不实现 Range 的布局测量，而 CodeMirror 的异步测量回调会调用
 * Range.getClientRects；缺少实现时会在 requestAnimationFrame 里抛错，
 * 把 worker 的 jsdom 全局状态带崩（后续测试文件会出现 localStorage/document
 * 等全局丢失的连锁失败）。这里补一个空实现，让测量路径安静返回。
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
