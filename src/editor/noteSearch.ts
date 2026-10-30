import { EditorState, type Extension } from "@codemirror/state";
import { EditorView, keymap } from "@codemirror/view";
import { highlightSelectionMatches, search, searchKeymap } from "@codemirror/search";

/**
 * 搜索面板的中文短语。键名必须与 @codemirror/search 源码里的 phrase() 调用一致，
 * 漏一条界面上就会残留英文（包含替换成功后的无障碍播报），所以整份都给出译文。
 */
const PHRASES = {
  Find: "查找",
  Replace: "替换",
  next: "下一个",
  previous: "上一个",
  all: "全部",
  "match case": "区分大小写",
  regexp: "正则表达式",
  "by word": "整词",
  replace: "替换",
  "replace all": "全部替换",
  close: "关闭",
  "Go to line": "跳转到行",
  go: "跳转",
  "current match": "当前匹配",
  "on line": "位于第",
  "replaced match on line $": "已替换第 $ 行的匹配",
  "replaced $ matches": "已替换 $ 处匹配",
};

/**
 * 搜索面板主题：只引用全局 CSS 变量，浅色与深色模式自动跟随应用配色；
 * 面板背景、输入框、按钮都接管，避免出现 CodeMirror 默认的灰底浏览器控件。
 */
const searchTheme = EditorView.theme({
  ".cm-panels": {
    backgroundColor: "var(--qc-bg-side)",
    color: "var(--qc-ink)",
    fontFamily: "var(--font-ui)",
  },
  ".cm-panels-top": {
    borderBottom: "1px solid var(--qc-hairline)",
  },
  ".cm-search": {
    display: "flex",
    flexWrap: "wrap",
    alignItems: "center",
    gap: "6px 8px",
    padding: "6px 10px 8px",
    fontSize: "12px",
    lineHeight: "1.4",
  },
  // 默认用 <br> 换行；在 flex 里让它独占一行，保持“查找一行、替换一行”的分组。
  ".cm-search br": { flexBasis: "100%", height: "0", margin: "0" },
  ".cm-search label": {
    display: "inline-flex",
    alignItems: "center",
    gap: "3px",
    fontSize: "11px",
    color: "var(--qc-ink-2)",
  },
  ".cm-search input[type=checkbox]": { accentColor: "var(--qc-accent)" },
  ".cm-textfield": {
    height: "24px",
    padding: "0 8px",
    border: "1px solid var(--qc-hairline)",
    borderRadius: "4px",
    backgroundColor: "var(--qc-bg-paper)",
    color: "var(--qc-ink)",
    fontSize: "12px",
    outline: "none",
    "&:focus": {
      borderColor: "var(--qc-accent)",
      boxShadow: "0 0 0 2px var(--qc-bg-active)",
    },
  },
  ".cm-button": {
    height: "24px",
    padding: "0 10px",
    border: "1px solid var(--qc-hairline)",
    borderRadius: "4px",
    backgroundImage: "none",
    backgroundColor: "var(--qc-bg-paper)",
    color: "var(--qc-ink-2)",
    fontSize: "12px",
    cursor: "pointer",
    "&:hover": {
      backgroundColor: "var(--qc-bg-hover)",
      color: "var(--qc-ink)",
    },
    "&:active": {
      backgroundColor: "var(--qc-bg-active)",
      backgroundImage: "none",
    },
  },
  ".cm-search button[name=close]": {
    padding: "0 4px",
    border: "none",
    backgroundColor: "transparent",
    color: "var(--qc-ink-3)",
    fontSize: "15px",
  },
  ".cm-searchMatch": {
    backgroundColor: "color-mix(in srgb, var(--qc-accent) 22%, transparent)",
    borderRadius: "2px",
  },
  ".cm-searchMatch.cm-searchMatch-selected": {
    backgroundColor: "color-mix(in srgb, var(--qc-accent) 45%, transparent)",
  },
  ".cm-selectionMatch": {
    backgroundColor: "var(--qc-marker)",
    outline: "1px solid var(--qc-accent-soft)",
  },
});

/**
 * 笔记内查找替换扩展：面板置顶、Ctrl+F/F3 快捷键、选中词高亮、中文文案与统一配色。
 * 只读由 CodeMirror 自己处理（replaceAll 命令和面板都会遵守 EditorState.readOnly），这里不加判断。
 */
export function noteSearchExtension(): Extension {
  return [
    search({ top: true }),
    keymap.of(searchKeymap),
    highlightSelectionMatches(),
    EditorState.phrases.of(PHRASES),
    searchTheme,
  ];
}
