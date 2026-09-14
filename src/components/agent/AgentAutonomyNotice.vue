<script setup lang="ts">
import { computed, ref } from "vue";
import type { AgentMessage } from "../../domain/agent";
import AgentText from "./AgentText.vue";
import VirtualScrollbar from "../VirtualScrollbar.vue";

/** 通知正文滚动容器：浮层滚动条与它同级。 */
const noticeScroll = ref<HTMLElement | null>(null);
const props = defineProps<{ message: AgentMessage }>();
const expanded = ref(false);
const kinds: Record<string, string> = { message: "收到追加消息", completed: "子任务本轮结束", failed: "子任务本轮失败", cancelled: "子任务本轮已中断" };
/** 只选取可信形状的展示字段，不把通知 JSON 或自动续轮提示当聊天正文。 */
const notice = computed(() => {
  if (props.message.kind === "goal_round") {
    const round = props.message.data?.round;
    return { title: typeof round === "number" ? "目标续轮 · 第 " + round + " 轮" : "目标继续执行", source: "", text: "这是目标的自动续轮，不是新的用户授权，也不表示目标已完成。" };
  }
  const raw = props.message.data?.notification;
  const data = raw && typeof raw === "object" ? raw as Record<string, unknown> : {};
  return {
    title: typeof data.kind === "string" ? kinds[data.kind] ?? "子任务状态更新" : "子任务状态更新",
    source: typeof data.agentId === "string" ? data.agentId : "",
    text: typeof data.content === "string" ? data.content : "通知详情不可用，可通过任务树查看子任务记录。",
  };
});
</script>
<template>
  <div class="autonomy-notice">
    <button type="button" :aria-expanded="expanded" @click="expanded = !expanded"><span>{{ expanded ? '▾' : '▸' }}</span>{{ notice.title }}<span class="hint">{{ expanded ? '收起' : '查看详情' }}</span></button>
    <div v-if="expanded" ref="noticeScroll" class="notice-body soft-scrollbar"><p v-if="notice.source" class="hint">来源：{{ notice.source }} · 子报告不代表根目标已验收</p><AgentText :text="notice.text" /></div>
    <VirtualScrollbar :target="noticeScroll" />
  </div>
</template>
<style scoped>
.autonomy-notice { border: 1px solid var(--color-hairline); border-radius: 8px; font-size: 12px; color: var(--color-ink-2); }
button { display: flex; align-items: center; gap: 8px; padding: 7px 10px; width: 100%; text-align: left; }
.hint { color: var(--color-ink-3); font-size: 11px; overflow-wrap: anywhere; }
button .hint { margin-left: auto; }
.notice-body { padding: 10px; border-top: 1px solid var(--color-hairline); max-height: 320px; overflow: auto; }
.notice-body :deep(.text-ink) { font-size: 13px; }
</style>
