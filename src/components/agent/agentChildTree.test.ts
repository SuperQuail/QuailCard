import { expect, test } from "vitest";
import type { AgentChildInfo } from "../../domain/agent";
import { childTreeRows, type ChildTreeRow } from "./agentChildTree";

/** 最小记录保留真实父身份，声明深度故意不可信。 */
function child(agentId: string, parentSessionId: string): AgentChildInfo {
  return { agentId, parentSessionId, delegationDepth: 99, description: agentId, status: "ready" };
}

// 同级顺序跟随身份首次出现的位置，但父关系与内容采用最后记录。
test("重复身份保持首次位置和最后内容，根身份不重复展示", () => {
  const latest = { ...child("a", "root"), description: "最新" };
  const rows = childTreeRows([child("a", "missing"), child("b", "root"), child("grand", "a"), latest, child("root", "a")], "root");
  expect(rows.map(row => [row.child.agentId, row.depth, row.detached])).toEqual([["a", 0, false], ["grand", 1, false], ["b", 0, false]]);
  expect(rows[0].child).toBe(latest);
});

// 环起点标异常，沿真实边可达的后代仍保留原有缩进语义。
test("缺父、自环、多节点环按既有优先级展示一次", () => {
  const rows = childTreeRows([child("a", "b"), child("b", "a"), child("self", "self"), child("leaf", "orphan"), child("orphan", "missing"), child("ok", "root")], "root");
  expect(rows.map(row => [row.child.agentId, row.depth, row.detached])).toEqual([
    ["ok", 0, false], ["orphan", 0, true], ["leaf", 1, false], ["a", 0, true], ["b", 1, false], ["self", 0, true],
  ]);
});

// 数万层链与环都必须使用显式栈，不能依赖 JS 调用栈深度。
test("三万层深链及闭环均无递归爆栈和重复行", () => {
  const count = 30_000;
  const nodes = Array.from({ length: count }, (_, index) => child(String(index), index ? String(index - 1) : "root"));
  const chain = childTreeRows(nodes, "root");
  expect(chain).toHaveLength(count); expect(chain[count - 1].depth).toBe(count - 1);
  nodes[0].parentSessionId = String(count - 1);
  const cycle = childTreeRows(nodes, "root");
  expect(cycle).toHaveLength(count); expect(cycle[0].detached).toBe(true);
  expect(cycle[count - 1].depth).toBe(count - 1);
});

// 计数属性访问而非耗时：旧实现反复全表扫描会产生 n² 级父身份读取。
test("宽树保持服务顺序且父身份读取次数为线性", () => {
  let reads = 0;
  const count = 10_000;
  const nodes = Array.from({ length: count }, (_, index) => ({
    ...child(String(index), "root"),
    /** 只计算算法查边次数，不将设备速度作为性能契约。 */
    get parentSessionId() { reads++; return "root"; },
  }));
  const rows = childTreeRows(nodes, "root");
  expect(rows.map(row => row.child.agentId)).toEqual(nodes.map(node => node.agentId));
  expect(reads).toBeLessThanOrEqual(count * 4);
});

/** 小图使用原算法作语义基准；规模受控，不用于性能测试。 */
function reference(children: AgentChildInfo[], rootId: string): ChildTreeRow[] {
  const rows: ChildTreeRow[] = [];
  const visited = new Set([rootId]);
  const unique = new Map(children.map(node => [node.agentId, node]));
  /** 复现原深度优先遍历，核验异常图的兼容性。 */
  function append(node: AgentChildInfo, depth: number, detached: boolean): void {
    if (visited.has(node.agentId)) return;
    visited.add(node.agentId); rows.push({ child: node, depth, detached });
    for (const next of unique.values()) if (next.parentSessionId === node.agentId) append(next, depth + 1, false);
  }
  for (const node of unique.values()) if (node.parentSessionId === rootId) append(node, 0, false);
  for (const node of unique.values()) if (!unique.has(node.parentSessionId)) append(node, 0, true);
  for (const node of unique.values()) append(node, 0, true);
  return rows;
}

// 固定种子覆盖父先/子先、重复、根重名、缺父与交错环，不使用随机时间种子。
test("多种异常小图与旧算法输出完全相同", () => {
  let seed = 17;
  /** 可复现的轻量序列只用于生成有限测试图。 */
  function pick(): string { seed = (seed * 1664525 + 1013904223) >>> 0; return String((seed >>> 16) % 10); }
  for (let round = 0; round < 100; round++) {
    const nodes = Array.from({ length: 16 }, () => child(pick(), pick()));
    expect(childTreeRows(nodes, "0")).toEqual(reference(nodes, "0"));
  }
});
