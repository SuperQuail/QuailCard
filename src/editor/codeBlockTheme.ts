import { EditorView } from "@codemirror/view";

/** 代码块与行内代码使用独立样式，正文换行策略不影响预览代码。 */
export const codeBlockTheme = EditorView.theme({
  ".qc-code-block": { margin: "12px 0", backgroundColor: "var(--qc-code-bg)", border: "1px solid var(--qc-hairline)", borderRadius: "6px", overflow: "hidden", fontFamily: "var(--font-mono)", fontSize: "0.9em" },
  ".qc-code-toolbar": { display: "flex", alignItems: "center", gap: "12px", padding: "6px 12px", borderBottom: "1px solid var(--qc-hairline)", fontFamily: "var(--font-ui)", fontSize: "11px", color: "var(--qc-ink-2)" },
  ".qc-code-toolbar span": { marginRight: "auto" },
  ".qc-code-toolbar button": { cursor: "pointer", padding: "2px 4px" },
  ".qc-code-toolbar button:focus-visible": { outline: "2px solid var(--qc-accent)" },
  ".qc-code-block pre": { margin: "0", padding: "12px", overflowX: "auto", whiteSpace: "pre", lineHeight: "1.65", tabSize: "4", minHeight: "44px" },
  ".qc-code-block pre.is-wrapped": { whiteSpace: "pre-wrap", overflowWrap: "anywhere" },
  ".qc-code-block code": { fontFamily: "inherit", fontSize: "inherit" },
  ".qc-code-line": { backgroundColor: "var(--qc-code-bg)", fontFamily: "var(--font-mono)", fontSize: "0.9em", lineHeight: "1.65", padding: "0 12px !important", minHeight: "1.65em" },
  ".qc-code-line .qc-md-code": { padding: "0", borderRadius: "0", fontSize: "inherit", background: "transparent" },
  ".tok-keyword, .tok-modifier": { color: "var(--qc-accent)" },
  ".tok-string, .tok-string2": { color: "var(--qc-success, #64815d)" },
  ".tok-number, .tok-bool, .tok-atom": { color: "var(--qc-warning, #ad7c36)" },
  ".tok-comment": { color: "var(--qc-ink-3)", fontStyle: "italic" },
  ".tok-typeName, .tok-className": { color: "var(--qc-accent)" },
});
