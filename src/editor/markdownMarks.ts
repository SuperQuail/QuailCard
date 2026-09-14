/**
 * Markdown 语法标记的统一规则表：正文与预览 widget 共用，避免同一段语法在两处表现不同。
 */

/**
 * 正文中隐藏的标记；决定显隐的是标记的父元素——选区与该元素相交时重新显示标记。
 * 这样点普通文字不会引起整行重排，只在正在编辑的元素上露出源码。
 */
export const BODY_MARK_NODES = new Set([
  "HeaderMark",
  "EmphasisMark",
  "QuoteMark",
  "LinkMark",
  "URL",
  "StrikethroughMark",
]);

/** 预览 widget 构造行内内容时跳过的标记：额外包含行内代码的反引号。 */
export const PREVIEW_MARK_NODES = new Set([...BODY_MARK_NODES, "CodeMark"]);
