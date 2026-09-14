<script setup lang="ts">
import { computed, ref, watch } from "vue";
import { RefreshCw, X } from "@lucide/vue";
import type { AgentChildInfo, AgentRun, AgentSession } from "../../domain/agent";
import AgentTranscript from "./AgentTranscript.vue";
import VirtualScrollbar from "../VirtualScrollbar.vue";

/** 详情滚动容器：浮层滚动条与它同级。 */
const detailScroll = ref<HTMLElement | null>(null);
const props = defineProps<{
  id: string; session: AgentSession | null; run: AgentRun | null; loading: boolean; error: string;
  rootId: string; children: AgentChildInfo[]; disabled?: boolean; managementError?: string;
}>();
const emit = defineEmits<{ openChild: [id: string]; closeChild: []; refreshChild: [];
  interruptChild: [id: string]; messageChild: [id: string, message: string] }>();
const draft = ref(""), validation = ref("");
const states = { running: "运行中", idle: "空闲", ready: "可恢复" };
/** 权限与状态只能来自宿主 children 事实，不能把历史 parent 字段当成消息授权。 */
const child = computed(() => props.children.find(item => item.agentId === props.id));
const direct = computed(() => !!props.rootId && props.id !== props.rootId && child.value?.parentSessionId === props.rootId);
/** 只提供已知父身份的查看导航，缺失关系只显示文字。 */
const canOpenParent = computed(() => !!child.value && (child.value.parentSessionId === props.rootId
  || props.children.some(item => item.agentId === child.value?.parentSessionId && item.agentId !== props.id)));
/** 返回根任务仅关闭详情，中间父任务继续使用只读查看事件。 */
function openParent(): void {
  if (!canOpenParent.value || !child.value) return;
  if (child.value.parentSessionId === props.rootId) emit("closeChild");
  else emit("openChild", child.value.parentSessionId);
}
const validSession = computed(() => props.session?.id === props.id ? props.session : null);
const validRun = computed(() => props.run?.sessionId === props.id ? props.run : null);
const title = computed(() => child.value?.description || validSession.value?.title || props.id);
/** 身份切换清空局部消息草稿，避免错发；父工作区草稿完全不参与。 */
watch(() => [props.id, props.rootId], () => { draft.value = ""; validation.value = ""; });
/** 提交时再次验证直属关系与大小；服务端仍须复验，展示组件不直接写入会话。 */
function sendMessage(): void {
  if (!direct.value || props.disabled) return;
  const message = draft.value.trim();
  if (!message) { validation.value = "请输入追加消息，不能只包含空白"; return; }
  if ([...draft.value].length > 16000) { validation.value = "追加消息不能超过 16000 字符"; return; }
  if (new TextEncoder().encode(draft.value).length > 16384) { validation.value = "追加消息的 UTF-8 大小不能超过 16 KiB"; return; }
  emit("messageChild", props.id, message); draft.value = ""; validation.value = "";
}
/** 中断仅针对 children 标记运行中的当前任务，不推断或级联后代。 */
function interrupt(): void { if (!props.disabled && child.value?.status === "running") emit("interruptChild", props.id); }
</script>
<template>
  <aside class="child-detail" aria-label="子任务详情">
    <header><div class="heading"><h2>{{ title }}</h2><p>{{ id }} · 只读查看</p></div>
      <button type="button" class="icon-btn" aria-label="刷新子任务详情" :disabled="loading" @click="emit('refreshChild')"><RefreshCw :size="16" /></button>
      <button type="button" class="icon-btn" aria-label="关闭子任务详情" @click="emit('closeChild')"><X :size="16" /></button>
    </header>
    <div class="status-bar"><span role="status">{{ child ? states[child.status] : '状态不可用 · 历史记录' }}</span>
      <button v-if="child?.status === 'running'" type="button" class="ghost-btn" :disabled="disabled" @click="interrupt">中断本轮</button>
    </div>
    <div v-if="child" class="parent-link"><span>直接父任务：{{ child.parentSessionId }}</span>
      <button v-if="canOpenParent" type="button" class="ghost-btn" @click="openParent">返回父任务</button>
    </div>
    <div ref="detailScroll" class="detail-scroll soft-scrollbar">
      <p v-if="loading" role="status">正在加载子任务历史…</p>
      <p v-if="error" role="alert">{{ error }}</p>
      <p v-if="managementError && managementError !== error" role="alert">{{ managementError }}</p>
      <p v-if="!validRun && validSession" class="hint">当前无实时轮次，显示已保存历史；不影响父任务。</p>
      <p v-if="!validSession && !validRun && !loading" class="hint">暂无可查看的子任务记录。</p>
      <AgentTranscript v-if="validSession || validRun" :id="id" :session="validSession" :run="validRun" @open-child="emit('openChild', $event)" />
    </div>
    <VirtualScrollbar :target="detailScroll" />
    <form v-if="direct" class="message-form" @submit.prevent="sendMessage">
      <label for="child-detail-message">给直接子任务追加消息</label>
      <textarea id="child-detail-message" v-model="draft" class="field-textarea" aria-label="详情追加消息" :disabled="disabled" rows="2" />
      <p v-if="validation" role="alert">{{ validation }}</p>
      <button type="submit" class="primary-btn" :disabled="disabled">追加消息</button>
      <p class="hint">只发送给该子任务，不替换父会话草稿。</p>
    </form>
    <p v-else class="relation-hint">仅可给直接子任务追加消息；后代任务请通过直接父任务协调。</p>
  </aside>
</template>
<style scoped>
.child-detail { display: flex; flex-direction: column; flex: 0 0 min(440px, 45%); min-width: 280px; min-height: 0; border-left: 1px solid var(--color-hairline); background: var(--color-bg); }
header { display: flex; align-items: center; gap: 6px; padding: 12px; } .heading { min-width: 0; flex: 1; } h2 { font-size: 14px; font-weight: 600; overflow-wrap: anywhere; }
.heading p, .hint, .relation-hint { font-size: 11px; color: var(--color-ink-3); overflow-wrap: anywhere; }
.status-bar { display: flex; align-items: center; justify-content: space-between; padding: 0 12px 10px; font-size: 12px; }
.detail-scroll { flex: 1; min-height: 0; overflow-y: auto; padding: 12px; } .detail-scroll > p { margin-bottom: 10px; }
.message-form { display: grid; gap: 6px; padding: 12px; border-top: 1px solid var(--color-hairline); font-size: 12px; }.message-form button { justify-self: end; }
.parent-link { display: flex; align-items: center; flex-wrap: wrap; gap: 4px; padding: 0 12px 8px; font-size: 11px; overflow-wrap: anywhere; }
.relation-hint { padding: 12px; } [role="alert"] { color: var(--color-danger); white-space: pre-wrap; overflow-wrap: anywhere; }
@media (max-width: 767px) { .child-detail { position: absolute; inset: 0 0 0 auto; z-index: 20; width: min(440px, 100%); min-width: 0; box-shadow: -4px 0 20px #0001; } }
</style>
