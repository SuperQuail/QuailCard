/** 保留三行上下文，展示完整替换区；无需对大文件执行平方复杂度比较。 */
export function agentDiff(before: string | null, after: string): Array<{ kind: string; text: string }> {
  const old = before === null ? [] : before.split("\n"), next = after.split("\n");
  let prefix = 0, suffix = 0;
  while (prefix < Math.min(old.length, next.length) && old[prefix] === next[prefix]) prefix++;
  while (suffix < Math.min(old.length, next.length) - prefix && old[old.length - suffix - 1] === next[next.length - suffix - 1]) suffix++;
  if (prefix === old.length && prefix === next.length) return [{ kind: "same", text: "内容没有变化" }];
  return [
    ...old.slice(Math.max(0, prefix - 3), prefix).map(text => ({ kind: "same", text })),
    ...old.slice(prefix, old.length - suffix).map(text => ({ kind: "remove", text })),
    ...next.slice(prefix, next.length - suffix).map(text => ({ kind: "add", text })),
    ...next.slice(next.length - suffix, next.length - suffix + 3).map(text => ({ kind: "same", text })),
  ];
}
