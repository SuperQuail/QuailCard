import { HighlightStyle, syntaxHighlighting } from "@codemirror/language";
import { tags as t } from "@lezer/highlight";
import { EditorView } from "@codemirror/view";

/** 编辑器基础主题：衬线排版、居中阅读列、跟随全局配色。 */
const baseTheme = EditorView.theme({
  "&": {
    height: "100%",
    backgroundColor: "transparent",
    color: "var(--qc-ink)",
  },
  ".cm-scroller": {
    fontFamily: "var(--font-serif)",
    fontSize: "var(--editor-font-size)",
    lineHeight: "var(--editor-line-height)",
    overflowY: "auto",
  },
  ".cm-content": {
    width: "100%",
    maxWidth: "880px",
    margin: "0 auto",
    padding: "40px 24px 96px",
    caretColor: "var(--qc-accent)",
  },
  ".cm-line": {
    padding: "0",
  },
  "&.cm-focused": {
    outline: "none",
  },
  ".cm-cursor, .cm-dropCursor": {
    borderLeftColor: "var(--qc-accent)",
  },
  ".cm-selectionBackground, ::selection": {
    backgroundColor: "var(--qc-bg-active) !important",
  },
  ".cm-gutters": {
    display: "none",
  },
  ".cm-activeLine": {
    backgroundColor: "var(--qc-accent-soft)",
    borderRadius: "1px",
    boxShadow: "0 0 0 3px var(--qc-accent-soft)",
  },
  ".cm-placeholder": {
    color: "var(--qc-ink-3)",
  },
  ".qc-image-preview": {
    display: "inline-flex",
    maxWidth: "100%",
    minHeight: "44px",
    alignItems: "center",
    color: "var(--qc-ink-3)",
    fontFamily: "var(--font-ui)",
    fontSize: "12px",
  },
  ".qc-image-preview img": {
    display: "block",
    maxWidth: "min(100%, 720px)",
    maxHeight: "480px",
    borderRadius: "6px",
    border: "1px solid var(--qc-hairline)",
    objectFit: "contain",
  },
  ".qc-image-preview.is-error": {
    color: "var(--qc-danger)",
  },
});

/** Markdown 语法高亮：标题放大、加粗倾斜生效、语法符号淡化为半透明。 */
const markdownHighlight = HighlightStyle.define([
  { tag: t.heading1, class: "qc-md-heading qc-md-heading-1" },
  { tag: t.heading2, class: "qc-md-heading qc-md-heading-2" },
  { tag: t.heading3, class: "qc-md-heading qc-md-heading-3" },
  { tag: t.strong, class: "qc-md-strong" },
  { tag: t.emphasis, class: "qc-md-em" },
  { tag: t.monospace, class: "qc-md-code" },
  { tag: t.link, class: "qc-md-link" },
  { tag: t.quote, class: "qc-md-quote" },
  { tag: t.strikethrough, class: "qc-md-strike" },
  { tag: t.processingInstruction, class: "qc-md-marker" },
]);

/** 深色模式下的链接与引文调整。 */
const darkTweaks = EditorView.theme(
  {
    ".qc-md-link": { color: "var(--qc-accent)" },
    ".qc-md-code": { backgroundColor: "var(--qc-code-bg)" },
  },
  { dark: true },
);

/** 浅色模式下的链接与引文调整。 */
const lightTweaks = EditorView.theme(
  {
    ".qc-md-link": { color: "var(--qc-accent)" },
    ".qc-md-code": { backgroundColor: "var(--qc-code-bg)" },
  },
  { dark: false },
);

/** 组装编辑器扩展主题（随深色模式切换）。 */
export function markdownTheme(dark: boolean) {
  return [baseTheme, syntaxHighlighting(markdownHighlight), dark ? darkTweaks : lightTweaks];
}
