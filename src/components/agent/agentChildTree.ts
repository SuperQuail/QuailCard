import type { AgentChildInfo } from "../../domain/agent";
export interface ChildTreeRow { child: AgentChildInfo; depth: number; detached: boolean }

/** 父映射与显式栈让构树保持 O(n)，且深链、损坏环和缺父记录都能安全展示。 */
export function childTreeRows(children: AgentChildInfo[], rootId: string): ChildTreeRow[] {
  const rows: ChildTreeRow[] = [];
  const visited = new Set<string>([rootId]);
  // Map 保留身份首次出现的位置，但内容采用最后一条，兼容既有重复身份语义。
  const unique = new Map(children.map(child => [child.agentId, child]));
  const byParent = new Map<string, AgentChildInfo[]>();
  for (const child of unique.values()) {
    const siblings = byParent.get(child.parentSessionId);
    if (siblings) siblings.push(child);
    else byParent.set(child.parentSessionId, [child]);
  }

  /** 逆序压栈复现原先深度优先的服务顺序，visited 让每个身份最多展开一次。 */
  function append(child: AgentChildInfo, depth: number, detached: boolean): void {
    if (visited.has(child.agentId)) return;
    const stack: ChildTreeRow[] = [{ child, depth, detached }];
    while (stack.length) {
      const row = stack.pop()!;
      if (visited.has(row.child.agentId)) continue;
      visited.add(row.child.agentId);
      rows.push(row);
      const descendants = byParent.get(row.child.agentId) ?? [];
      for (let index = descendants.length - 1; index >= 0; index--) {
        stack.push({ child: descendants[index], depth: row.depth + 1, detached: false });
      }
    }
  }

  for (const child of byParent.get(rootId) ?? []) append(child, 0, false);
  for (const child of unique.values()) if (!unique.has(child.parentSessionId)) append(child, 0, true);
  for (const child of unique.values()) append(child, 0, true);
  return rows;
}
