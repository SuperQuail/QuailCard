/**
 * 文档类型：同一 Vault 里可以并存的两种用户文件。
 *
 * 判定规则集中在扩展名上，后端 vaultfs/documents.rs 与这里必须给出相同结果：
 * 大小写不敏感，其它扩展名一律按 Markdown 处理（与历史数据一致）。
 */
export type DocumentKind = "markdown" | "wordGroup";

/** Markdown 笔记扩展名。 */
export const MARKDOWN_EXTENSION = ".md";

/** 单词组扩展名：JSON 文件，标题即文件名去该后缀。 */
export const WORD_GROUP_EXTENSION = ".qword";

/** 路径的文档类型；没有 .qword 后缀时按 Markdown 处理。 */
export function documentKindOf(path: string): DocumentKind {
  return path.toLowerCase().endsWith(WORD_GROUP_EXTENSION) ? "wordGroup" : "markdown";
}

/** 是否为单词组文件：图标、过滤与链接候选共用，避免各处重复写 endsWith。 */
export function isWordGroupPath(path: string): boolean {
  return documentKindOf(path) === "wordGroup";
}

/** 路径的文档扩展名（带点）；未知扩展名返回空串，用于改名时保留原类型。 */
export function documentExtensionOf(path: string): string {
  const lower = path.toLowerCase();
  if (lower.endsWith(WORD_GROUP_EXTENSION)) return WORD_GROUP_EXTENSION;
  if (lower.endsWith(MARKDOWN_EXTENSION)) return MARKDOWN_EXTENSION;
  return "";
}

/** 去掉路径末尾的已知文档扩展名；未知扩展名原样保留，大小写不敏感。 */
export function stripDocumentExtension(path: string): string {
  const extension = documentExtensionOf(path);
  return extension ? path.slice(0, -extension.length) : path;
}

/** 路径对应的标题：取最后一段并去掉文档扩展名。 */
export function documentTitleOf(path: string): string {
  const name = path.split(/[\\/]/).pop() ?? path;
  return stripDocumentExtension(name);
}
