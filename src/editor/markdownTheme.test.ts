import { markdown, markdownLanguage } from "@codemirror/lang-markdown";
import { EditorState } from "@codemirror/state";
import { EditorView } from "@codemirror/view";
import { expect, test } from "vitest";
import { markdownTheme } from "./markdownTheme";

/** 六级标题都要有各自的样式类，否则高等级标题看起来和正文一样。 */
test("一到六级标题都套用各自的标题样式类", () => {
  const view = new EditorView({
    parent: document.body,
    state: EditorState.create({
      doc: "# 一级\n\n### 三级\n\n###### 六级",
      extensions: [markdown({ base: markdownLanguage }), markdownTheme(false)],
    }),
  });
  try {
    for (const level of [1, 3, 6]) {
      expect(view.dom.querySelector(`.qc-md-heading-${level}`)).not.toBeNull();
    }
  } finally { view.destroy(); }
});
