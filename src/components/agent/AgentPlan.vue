<script setup lang="ts">
import { computed, ref, useId } from "vue";
import { ChevronDown } from "@lucide/vue";
import AgentEvidence from "./AgentEvidence.vue";
import type { AgentMessage, GoalEvidence, Plan, PlanStatus } from "../../domain/agent";

/**
 * 正文里的一行折叠卡：折叠时只留「计划 1/5 · 当前进行项」一行，
 * 展开才铺开五态明细与结果依据；计划完成不代表目标已验收。
 * plan 是当前计划，message 兼容旧会话的 update_plan 消息载荷。
 */
const props = defineProps<{ plan?: Plan; message?: AgentMessage; evidence?: GoalEvidence[]; criteria?: string[] }>();
const emit = defineEmits<{ openChild: [id: string] }>();
/** 引用只按宿主收据精确匹配，不能从文本猜路径或子任务身份。 */
function sourceEvidence(reference: string): GoalEvidence[] { return props.evidence?.filter(item => item.receiptRef === reference) ?? []; }
const bodyId = "agent-plan-body-" + useId();
const expanded = ref(false);
const labels: Record<PlanStatus, string> = { pending: "待开始", in_progress: "进行中", completed: "已完成", blocked: "受阻", cancelled: "已取消" };
/** 计数摘要固定顺序，只列非零项，避免「0 已完成」这类噪音。 */
const order: PlanStatus[] = ["in_progress", "pending", "completed", "blocked", "cancelled"];
/** 当前计划优先；兼容旧消息的 text 字段，但不把未知状态当成成功。 */
const steps = computed(() => {
  if (props.plan) return props.plan.steps;
  const raw = props.message?.data?.steps;
  if (!Array.isArray(raw)) return [];
  return raw.flatMap((item, index) => {
    if (!item || typeof item !== "object") return [];
    const content = typeof item.content === "string" ? item.content : item.text;
    if (typeof content !== "string" || !Object.prototype.hasOwnProperty.call(labels, item.status)) return [];
    return [{ id: String(item.id ?? index), content, status: item.status as PlanStatus,
      childAgentId: typeof item.childAgentId === "string" ? item.childAgentId : null,
      resultRefs: Array.isArray(item.resultRefs) ? item.resultRefs.filter((ref: unknown): ref is string => typeof ref === "string") : [] }];
  });
});
/** 仅 completed 计入完成数量，取消与受阻保留独立语义。 */
const counts = computed(() => {
  const total: Record<PlanStatus, number> = { pending: 0, in_progress: 0, completed: 0, blocked: 0, cancelled: 0 };
  for (const step of steps.value) total[step.status] += 1;
  return total;
});
const completed = computed(() => counts.value.completed);
/** 并行进行项：折叠头只展示第一项，其余用尾标计数，尾标不参与省略。 */
const parallel = computed(() => Math.max(0, counts.value.in_progress - 1));
/** 当前项优先取进行中，其次取第一个待开始；没有可做项时不拼多余的尾巴。 */
const current = computed(() => (steps.value.find(step => step.status === "in_progress") ?? steps.value.find(step => step.status === "pending"))?.content ?? "");
/** 展开后的计数摘要只列非零项，语义与五态标签一致。 */
const summary = computed(() => order.filter(status => counts.value[status] > 0).map(status => counts.value[status] + " " + labels[status]).join(" · "));
</script>
<template>
  <section v-if="steps.length" class="agent-plan" aria-label="当前执行计划">
    <button type="button" class="plan-head" :aria-expanded="expanded" :aria-controls="expanded ? bodyId : undefined" @click="expanded = !expanded">
      <span class="plan-label">计划 {{ completed }}/{{ steps.length }}<template v-if="current"> · {{ current }}</template></span>
      <span v-if="parallel > 0" class="plan-more" :title="'另有 ' + parallel + ' 项并行进行中'">+{{ parallel }}</span>
      <ChevronDown class="chevron" :data-open="expanded || undefined" :size="14" />
    </button>
    <div v-if="expanded" :id="bodyId" class="plan-body">
      <p class="plan-summary">{{ summary }}</p>
      <ol>
        <li v-for="step in steps" :key="step.id" :data-state="step.status">
          <span class="step-status">{{ labels[step.status] }}</span>
          <div class="step-content"><span>{{ step.content }}</span>
            <button v-if="step.childAgentId" type="button" class="child-link" @click="emit('openChild', step.childAgentId)">查看子任务 · {{ step.childAgentId }}</button>
            <details v-if="step.resultRefs.length"><summary>结果依据 {{ step.resultRefs.length }}</summary>
              <ul><li v-for="reference in step.resultRefs" :key="reference">
                <span>{{ reference }}</span>
                <AgentEvidence v-if="sourceEvidence(reference).length" :evidence="sourceEvidence(reference)" :criteria="criteria" />
                <details v-else><summary>查看来源信息</summary><p>当前会话无可验证来源信息；引用仅作文本展示，不执行跳转。</p></details>
              </li></ul>
            </details>
          </div>
        </li>
      </ol>
      <p class="plan-hint">计划完成不代表目标已验收</p>
    </div>
  </section>
</template>
<style scoped>
.agent-plan { font-size: 13px; line-height: 1.7; }
.plan-head { display: flex; align-items: center; gap: 8px; width: 100%; padding: 6px 10px; border: 1px solid var(--color-hairline); border-radius: 8px; background: var(--color-bg-side); color: var(--color-ink-2); text-align: left; transition: background-color 120ms, color 120ms; }
.plan-head:hover { color: var(--color-ink); }
.plan-head[aria-expanded="true"] { border-bottom-left-radius: 0; border-bottom-right-radius: 0; color: var(--color-ink); }
.plan-label { flex: 1; min-width: 0; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; font-size: 12px; }
.plan-more { flex-shrink: 0; font-size: 11px; color: var(--color-ink-3); }
.chevron { flex-shrink: 0; transition: transform 120ms; }
.chevron[data-open] { transform: rotate(180deg); }
.plan-body { padding: 8px 10px 10px; border: 1px solid var(--color-hairline); border-top: 0; border-radius: 0 0 8px 8px; }
.plan-summary { margin-bottom: 6px; color: var(--color-ink-2); font-size: 11px; }
ol { display: grid; gap: 6px; }
ol > li { display: flex; align-items: baseline; gap: 10px; }
.step-status { flex-shrink: 0; font-size: 11px; color: var(--color-ink-2); }
.child-link { display: block; color: var(--color-accent-strong); font-size: 12px; text-align: left; }
.child-link:hover { text-decoration: underline; }
.step-content { min-width: 0; overflow-wrap: anywhere; }
[data-state="in_progress"] .step-status { color: var(--color-accent-strong); font-weight: 600; }
[data-state="blocked"] .step-status { color: var(--color-danger); }
[data-state="completed"], [data-state="cancelled"] { color: var(--color-ink-3); }
[data-state="cancelled"] .step-content > span { text-decoration: line-through; }
.plan-hint { margin-top: 6px; color: var(--color-ink-3); font-size: 11px; }
summary { cursor: pointer; color: var(--color-ink-2); font-size: 12px; }
</style>
