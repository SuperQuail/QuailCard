import {
  autocompletion,
  type Completion,
  type CompletionContext,
  type CompletionResult,
  type CompletionSource,
  pickedCompletion,
} from "@codemirror/autocomplete";
import { syntaxTree } from "@codemirror/language";
import type { EditorState, Extension } from "@codemirror/state";
import type { EditorView } from "@codemirror/view";
import type { LinkTarget } from "../domain/links";

/**
 * 补全候选：标题与别名来自调用方注入的摘要；mtime 可选，
 * 给 NoteSummary 数组时按"最近修改优先"排序，只给最小目标时退回按标题排序。
 */
export type CompletionTarget = LinkTarget & { mtime?: number };

/** 双链内不参与补全的语法节点：代码、frontmatter 与既有双链都不该被改写。 */
const SKIPPED_NODES = new Set(["InlineCode", "FencedCode", "CodeBlock", "Frontmatter", "WikiLink"]);

/** 候选上限：轻量补全不给输入过程制造长列表。 */
const MAX_OPTIONS = 50;

/** 去掉路径末段扩展名，得到可直接写进 [[ ]] 的路径形式。 */
function pathStem(path: string): string {
  return path.replace(/\\/g, "/").replace(/\.md$/i, "");
}

/** 标题归一化：比较补全冲突时不区分大小写与首尾空白。 */
function titleKey(value: string): string {
  return value.trim().toLowerCase();
}

/** 光标是否位于代码块、行内代码、frontmatter 或既有双链内部。 */
function insideSkippedNode(state: EditorState, position: number): boolean {
  let node: ReturnType<typeof syntaxTree>["topNode"] | null = syntaxTree(state).resolveInner(position, -1);
  while (node) {
    if (SKIPPED_NODES.has(node.name)) return true;
    node = node.parent;
  }
  return false;
}

/**
 * 生成插入动作：光标停在 ]] 之前；后面已经有闭合方括号时不再重复补，
 * 这样在 [[目标]] 里补全显示名不会变成 [[...]]]]。
 */
function insertOnApply(text: string): Completion["apply"] {
  return (view: EditorView, completion: Completion, from: number, to: number) => {
    const closing = view.state.sliceDoc(to, Math.min(to + 2, view.state.doc.length)) === "]]" ? "" : "]]";
    view.dispatch({
      changes: { from, to, insert: `${text}${closing}` },
      selection: { anchor: from + text.length },
      annotations: pickedCompletion.of(completion),
    });
  };
}

/** 补全候选的选择结果：列表标签与插入文本分离，标签只用于匹配与展示。 */
interface Candidate {
  label: string;
  mtime: number;
  completion: Completion;
}

/**
 * 构建候选列表。
 *
 * 契约：
 * - 输入 [[ 之后按已输入文字对标题与别名做大小写不敏感的"包含"匹配；
 * - 按笔记修改时间新到旧排序（无 mtime 时按标题），最多返回 MAX_OPTIONS 条；
 * - 同名标题有冲突时改用路径形式 [[dir/标题]]，保证插入后一定可解析；
 * - 别名候选插入 [[标题|别名]]，显示名就是用户输入的别名。
 */
function buildOptions(context: CompletionContext, targets: CompletionTarget[]): CompletionResult | null {
  const match = context.matchBefore(/\[\[[^\[\]\n|#]*$/);
  if (!match) return null;
  if (insideSkippedNode(context.state, context.pos)) return null;
  const typedFrom = match.from + 2;
  const query = context.state.doc.sliceString(typedFrom, context.pos).trim().toLowerCase();

  const titleCounts = new Map<string, number>();
  for (const target of targets) {
    const key = titleKey(target.title || pathStem(target.path));
    titleCounts.set(key, (titleCounts.get(key) ?? 0) + 1);
  }

  const candidates: Candidate[] = [];
  for (const target of targets) {
    const title = (target.title || pathStem(target.path)).trim();
    if (!title) continue;
    const ambiguous = (titleCounts.get(titleKey(title)) ?? 0) > 1;
    const insertBase = ambiguous ? pathStem(target.path) : title;
    const mtime = target.mtime ?? 0;
    if (title.toLowerCase().includes(query)) {
      candidates.push({ label: title, mtime, completion: {
        label: title, detail: target.path, type: "text", apply: insertOnApply(insertBase),
      } });
    }
    for (const alias of target.aliases ?? []) {
      const name = alias.trim();
      if (!name || !name.toLowerCase().includes(query)) continue;
      candidates.push({ label: name, mtime, completion: {
        label: name, detail: `${target.path} · 别名`, type: "text", apply: insertOnApply(`${insertBase}|${name}`),
      } });
    }
  }

  const options = candidates
    .sort((left, right) => right.mtime - left.mtime || left.label.localeCompare(right.label, "zh-CN"))
    .slice(0, MAX_OPTIONS)
    .map((candidate) => candidate.completion);
  if (options.length === 0) return null;
  // filter: false —— 匹配与排序已在这里完成，保留 mtime 顺序不被自动排序打乱。
  return { from: typedFrom, to: context.pos, options, filter: false };
}

/** 补全源独立导出，测试可脱离自动补全扩展直接调用。 */
export function wikilinkCompletionSource(getTargets: () => CompletionTarget[]): CompletionSource {
  return (context) => buildOptions(context, getTargets());
}

/** [[ 触发的双链补全扩展；候选数据由宿主注入，组件不直接读后端。 */
export function wikilinkCompletion(getTargets: () => CompletionTarget[]): Extension {
  return autocompletion({
    override: [wikilinkCompletionSource(getTargets)],
    activateOnTyping: true,
  });
}
