<script setup lang="ts">
import { computed } from "vue";
import type { AgentMessage, AgentRun, AgentSession } from "../../domain/agent";
import AgentAutonomyNotice from "./AgentAutonomyNotice.vue";
import AgentDraftSummary from "./AgentDraftSummary.vue";
import AgentEvidence from "./AgentEvidence.vue";
import AgentPlan from "./AgentPlan.vue";
import AgentSafeToolCalls from "./AgentSafeToolCalls.vue";
const props = defineProps<{ id: string; session: AgentSession | null; run: AgentRun | null }>();
const emit = defineEmits<{ openChild: [id: string] }>();
/** 迟到的其他子会话快照不能污染当前详情；历史加载前仍能显示匹配的实时轮次。 */
const session = computed(() => props.session?.id === props.id ? props.session : null);
const run = computed(() => props.run?.sessionId === props.id ? props.run : null);
/** 历史与流式同身份合并，实时较长文本可覆盖已落盘前缀，轮次移除后自然回退历史。 */
const messages = computed(() => {
  const all = session.value?.messages ?? [];
  const lastPlan = [...all].reverse().find(message => message.kind === "plan")?.id;
  const result = all.filter(message => !["running", "reasoning"].includes(message.kind)
    && (message.kind !== "plan" || (!session.value?.plan && message.id === lastPlan)));
  const live = run.value;
  if (!live?.text) return result;
  const index = result.findIndex(message => live.textMessageId ? message.id === live.textMessageId : message.content === live.text);
  if (index < 0) return [...result, { id: live.textMessageId || live.id + "-stream", role: "assistant", kind: "text", content: live.text, data: null }];
  return result.map((message, i) => i === index && live.text.length > message.content.length ? { ...message, content: live.text } : message);
});
/** 只读注册表隔离带写入动作的复习、草稿、笔记与视频组件。 */
const renderers = { tool_calls: AgentSafeToolCalls, exchange: AgentSafeToolCalls, drafts: AgentDraftSummary,
  agent_message: AgentAutonomyNotice, goal_round: AgentAutonomyNotice };
/** 未注册类型只显示安全通用正文，未知 data 永不序列化。 */
function renderer(message: AgentMessage) { return renderers[message.kind as keyof typeof renderers]; }
</script>
<template>
  <div class="agent-transcript" aria-label="子任务只读记录">
    <section v-if="session?.goal" class="goal-summary"><p>目标：{{ session.goal.objective }}</p>
      <AgentEvidence :evidence="session.goal.evidence" :criteria="session.goal.acceptanceCriteria" />
    </section>
    <AgentPlan v-if="session?.plan?.steps.length" :plan="session.plan" :evidence="session.goal?.evidence" :criteria="session.goal?.acceptanceCriteria" @open-child="emit('openChild', $event)" />
    <p v-if="!messages.length" class="hint">暂无消息记录。</p>
    <article v-for="message in messages" :key="message.id" :data-message-id="message.id">
      <span class="role">{{ message.role === 'user' ? '用户' : '子 Agent' }}</span>
      <AgentPlan v-if="message.kind === 'plan'" :message="message" :evidence="session?.goal?.evidence" :criteria="session?.goal?.acceptanceCriteria" @open-child="emit('openChild', $event)" />
      <component :is="renderer(message)" v-else-if="renderer(message)" :message="message" />
      <template v-else><p v-if="message.kind === 'review'" class="hint">复习记录 · 仅查看，不启动复习</p><p class="body">{{ message.content }}</p></template>
    </article>
    <p v-if="run?.phase" class="hint" role="status">当前轮次：{{ run.phase }}</p>
    <p v-if="run?.error" role="alert">{{ run.error }}</p>
  </div>
</template>
<style scoped>
.agent-transcript { display: grid; gap: 18px; min-width: 0; }
.role, .hint { color: var(--color-ink-3); font-size: 11px; }.role { display: block; margin-bottom: 6px; }
.body, .goal-summary { white-space: pre-wrap; overflow-wrap: anywhere; font-size: 13px; line-height: 1.8; }
[role="alert"] { color: var(--color-danger); overflow-wrap: anywhere; }
</style>
