<script setup lang="ts">
import { computed } from "vue";
import AgentEvidence from "./AgentEvidence.vue";
import type { AgentRunState, Goal } from "../../domain/agent";
import { goalPhaseHint, goalPhaseWord } from "./agentGoalPhase";

/**
 * 目标下拉的内容：它是浮层正文，不再带常驻横幅边框；
 * 计划改由聊天正文的折叠卡渲染，这里不重复步骤清单。
 * 只呈现宿主事实，不根据计划勾选推断目标完成。
 */
const props = defineProps<{
  goal: Goal | null; goalPhase: string | null; runState: AgentRunState | null;
  waitingReason: string | null; currentPhase?: string; busy: boolean; disabled?: boolean; canResume?: boolean;
}>();
const emit = defineEmits<{ stop: []; resumeGoal: [] }>();
const waits: Record<string, string> = { waitingChildren: "等待子任务结果", waitingUser: "等待你确认或采纳" };
/** 阶段短词与头部 chip 同源，避免同一事实出现两种说法。 */
const phase = computed(() => goalPhaseWord(props.goalPhase, props.runState, props.busy, props.goal?.phase));
/** 长解释只作为展开后的 hint，绝不当作标题文字。 */
const hint = computed(() => goalPhaseHint(props.goalPhase, props.runState, props.busy));
/** 等待与阻塞是宿主事实；没有等待原因时回落到持久化的阻塞原因。 */
const reason = computed(() => props.waitingReason ? waits[props.waitingReason] ?? props.waitingReason : props.goal?.blocker?.reason ?? "");
/** 目标完成后没有可取消的轮次，隐藏暂停/停止/继续。 */
const pending = computed(() => props.goalPhase !== "complete");
</script>
<template>
  <div v-if="goal" class="goal-panel" aria-label="目标">
    <div class="goal-toolbar">
      <span class="phase" role="status" :title="hint || undefined">{{ phase }}</span>
      <span class="rounds">已启动 {{ goal.roundsStarted }} 轮</span>
      <div v-if="pending" class="controls">
        <button v-if="busy" type="button" class="ghost-btn" :disabled="disabled" @click="emit('stop')">暂停</button>
        <button v-else type="button" class="ghost-btn" :disabled="disabled || !canResume" @click="emit('resumeGoal')">继续目标</button>
        <button type="button" class="ghost-btn" :disabled="disabled" @click="emit('stop')">停止全部</button>
      </div>
    </div>
    <p v-if="hint" class="hint">{{ hint }}</p>
    <p v-if="reason" class="reason" role="status">{{ reason }}</p>
    <p v-if="currentPhase" class="current-step">当前步骤：{{ currentPhase }}</p>
    <details v-if="goal.acceptanceCriteria.length" class="criteria">
      <summary>验收条件 {{ goal.acceptanceCriteria.length }} · 收据 {{ goal.evidence.length }}</summary>
      <ol>
        <li v-for="(criterion, index) in goal.acceptanceCriteria" :key="index">{{ criterion }}</li>
      </ol>
    </details>
    <AgentEvidence :evidence="goal.evidence" :criteria="goal.acceptanceCriteria" />
  </div>
</template>
<style scoped>
.goal-panel { font-size: 12px; }
.goal-toolbar { display: flex; align-items: center; flex-wrap: wrap; gap: 8px; }
.phase { font-weight: 600; color: var(--color-ink); }
.rounds { color: var(--color-ink-2); }
.controls { display: flex; gap: 4px; margin-left: auto; }
.controls button { height: 28px; padding: 0 8px; font-size: 12px; }
.hint { margin-top: 6px; color: var(--color-ink-3); font-size: 11px; overflow-wrap: anywhere; }
.reason, .current-step { margin-top: 6px; color: var(--color-ink-2); overflow-wrap: anywhere; }
.criteria { margin-top: 6px; }
summary { padding: 4px 0; cursor: pointer; color: var(--color-ink-2); overflow-wrap: anywhere; }
.criteria ol { display: grid; gap: 4px; padding-left: 20px; list-style: decimal; overflow-wrap: anywhere; }
</style>
