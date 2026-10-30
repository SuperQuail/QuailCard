import { CompletionContext, type CompletionResult } from "@codemirror/autocomplete";
import { markdown, markdownLanguage } from "@codemirror/lang-markdown";
import { EditorState } from "@codemirror/state";
import { EditorView } from "@codemirror/view";
import { describe, expect, test } from "vitest";
import { wikilinkCompletion, wikilinkCompletionSource, type CompletionTarget } from "./wikilinkCompletion";

/** 构造最小候选；mtime 用于验证"最近修改优先"的排序契约。 */
function target(path: string, title: string, aliases: string[] = [], mtime = 0): CompletionTarget {
  return { path, title, aliases, mtime };
}

/** 按文档与光标位置运行补全源，返回结果（不做自动补全的 UI 交互）。 */
async function complete(doc: string, targets: CompletionTarget[], pos = doc.length) {
  const state = EditorState.create({ doc, extensions: [markdown({ base: markdownLanguage })] });
  const result = await wikilinkCompletionSource(() => targets)(new CompletionContext(state, pos, false));
  return result;
}

/** 执行候选的 apply，返回插入后的文档文本。 */
function applyOption(doc: string, result: CompletionResult, index: number): string {
  const view = new EditorView({
    parent: document.body,
    state: EditorState.create({ doc, extensions: [markdown({ base: markdownLanguage })] }),
  });
  try {
    const option = result.options[index]!;
    (option.apply as (view: EditorView, completion: typeof option, from: number, to: number) => void)(
      view, option, result.from, result.to ?? view.state.doc.length);
    return view.state.doc.toString();
  } finally {
    view.destroy();
  }
}

describe("双链补全", () => {
  test("输入 [[ 触发，候选按 mtime 新到旧排序", async () => {
    const result = await complete("见 [[", [
      target("old.md", "旧笔记", [], 1),
      target("new.md", "新笔记", [], 5),
    ]);
    expect(result).not.toBeNull();
    expect(result!.from).toBe(4);
    expect(result!.options.map((option) => option.label)).toEqual(["新笔记", "旧笔记"]);
  });

  test("没有 [[ 前缀或位于代码/frontmatter/既有双链内不触发", async () => {
    expect(await complete("普通文字", [target("a.md", "A")])).toBeNull();
    expect(await complete("`[[x`", [target("a.md", "A")], 3)).toBeNull();
    const inFrontmatter = "---\n[[x\n---\n";
    expect(await complete(inFrontmatter, [target("a.md", "A")], inFrontmatter.indexOf("x") + 1)).toBeNull();
    expect(await complete("[[B]]", [target("a.md", "A")], 3)).toBeNull();
  });

  test("按标题包含匹配，别名作为独立候选并标注来源", async () => {
    const result = await complete("[[se", [
      target("deep.md", "Deep"),
      target("b.md", "B", ["Sea"]),
    ]);
    expect(result!.options.map((option) => option.label)).toEqual(["Sea"]);
    expect(result!.options[0]!.detail).toContain("b.md");
    expect(result!.options[0]!.detail).toContain("别名");
  });

  test("最多返回 50 条", async () => {
    const targets = Array.from({ length: 60 }, (_, index) => target(`n${index}.md`, `笔记${index}`));
    const result = await complete("[[", targets);
    expect(result!.options).toHaveLength(50);
  });

  test("唯一标题补成 [[标题]]，同名冲突补成路径形式", async () => {
    const unique = await complete("[[B", [target("dir/B.md", "B")]);
    expect(applyOption("[[B", unique!, 0)).toBe("[[B]]");
    const conflict = await complete("[[同", [
      target("a/同名.md", "同名", [], 1),
      target("b/同名.md", "同名", [], 5),
    ]);
    // 新修改的 b 排在前面，插入的是路径形式而不是有歧义的标题。
    expect(applyOption("[[同", conflict!, 0)).toBe("[[b/同名]]");
  });

  test("别名候选补成 [[标题|别名]]，光标停在闭合方括号前", async () => {
    const result = await complete("[[Be", [target("b.md", "B", ["Bee"])]);
    const view = new EditorView({
      parent: document.body,
      state: EditorState.create({ doc: "[[Be", extensions: [markdown({ base: markdownLanguage })] }),
    });
    try {
      const option = result!.options[0]!;
      (option.apply as (view: EditorView, completion: typeof option, from: number, to: number) => void)(
        view, option, result!.from, result!.to!);
      expect(view.state.doc.toString()).toBe("[[B|Bee]]");
      expect(view.state.selection.main.head).toBe("[[B|Bee".length);
    } finally {
      view.destroy();
    }
  });

  test("已有闭合方括号时不重复补 ]]", async () => {
    const result = await complete("[[Be", [target("b.md", "B", ["Bee"])]);
    const view = new EditorView({
      parent: document.body,
      state: EditorState.create({ doc: "[[Be]]", extensions: [markdown({ base: markdownLanguage })] }),
    });
    try {
      const option = result!.options[0]!;
      (option.apply as (view: EditorView, completion: typeof option, from: number, to: number) => void)(
        view, option, 2, 4);
      expect(view.state.doc.toString()).toBe("[[B|Bee]]");
    } finally {
      view.destroy();
    }
  });

  test("补全扩展工厂返回可挂载的扩展", () => {
    expect(wikilinkCompletion(() => [])).toBeTruthy();
  });

  test("![[ 内嵌形式同样触发补全", async () => {
    const result = await complete("![[", [target("a.md", "A")]);
    expect(result).not.toBeNull();
    expect(result!.from).toBe(3);
  });
});
