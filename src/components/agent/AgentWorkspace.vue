<script setup lang="ts">
import { computed, onBeforeUnmount, ref, watch } from "vue";
import { Bot, History, Plus, X, Brain } from "@lucide/vue";
import { projectAgentAutonomy, projectAgentAutonomyRun } from "../../domain/agentAutonomy";
import type { AgentChildDetailState, AgentChildInfo, AgentImage, AgentMessage, AgentRun, AgentSession } from "../../domain/agent";
import AgentImages from "./AgentImages.vue";
import AgentVideo from "./AgentVideo.vue";
import type { NoteSummary, ProviderSummary } from "../../domain/types";
import type { ReviewSessionFlow } from "../../services/reviewSession";
import AgentAction from "./AgentAction.vue";
import AgentText from "./AgentText.vue";
import AgentReview from "./AgentReview.vue";
import AgentDrafts from "./AgentDrafts.vue";
import AgentComposer from "./AgentComposer.vue";
import AgentHistory from "./AgentHistory.vue";
import AgentReasoning from "./AgentReasoning.vue";
import AgentSafeToolCalls from "./AgentSafeToolCalls.vue";
import AgentChildDetail from "./AgentChildDetail.vue";
import AgentPlan from "./AgentPlan.vue";
import AgentHeaderChips from "./AgentHeaderChips.vue";
import AgentAutonomyNotice from "./AgentAutonomyNotice.vue";
import VirtualScrollbar from "../VirtualScrollbar.vue";
const props = defineProps<{
  session: AgentSession | null; sessions: AgentSession[]; run: AgentRun | null; draft: string;
  selectedPaths: string[]; providerId: string; providers: ProviderSummary[]; notes: NoteSummary[];
  memory: string; error: string; loading: boolean; sending: boolean; aiGrading: boolean;
  suspended?: boolean;
  children?: AgentChildInfo[]; childrenError?: string;
  childDetail?: AgentChildDetailState | null;
  images?: AgentImage[]; readingImages?: boolean;
  reviewFlow: (message: AgentMessage) => ReviewSessionFlow;
}>();
const emit = defineEmits<{
  send: [text: string]; stop: []; newSession: []; session: [id: string]; close: []; settings: [];
  draft: [text: string]; scope: [paths: string[]]; deleteSession: [id: string]; action: [name: string, value: string];
  memory: [content: string];
  pasteImages: [files: File[]]; removeImage: [index: number];
  adopt: [messageId: string, ids: string[]];
  openChild: [id: string]; closeChild: []; refreshChild: [];
  refreshChildren: []; interruptChild: [id: string];
  messageChild: [id: string, message: string]; resumeGoal: [];
}>();
const historyOpen = ref(false), memoryOpen = ref(false);
const memoryDraft = ref("");
const scroll = ref<HTMLElement | null>(null), outerScroll = ref<HTMLElement | null>(null), follow = ref(true), selectedReview = ref("");
/** 丢弃其他会话的运行快照，避免旧任务误锁当前输入与目标操作。 */
const savedAutonomy = computed(() => projectAgentAutonomy(props.session, null));
/** 实时状态只叠加运行字段，静态目标与计划不因每个 token 重新复制。 */
const autonomy = computed(() => projectAgentAutonomyRun(savedAutonomy.value, props.session?.id, props.run));
const running = computed(() => autonomy.value.busy || props.sending);
/** 实时思考行只在推理还没落盘时显示，落盘后由历史消息接管，避免重复。 */
const showReasoning = computed(() => {
  const run = props.run;
  if (!run?.reasoning || run.sessionId !== props.session?.id) return false;
  return !props.session?.messages.some(message => message.kind === "reasoning" && message.id === run.reasoningMessageId);
});
const ready = computed(() => props.providers.some(p => p.id === props.providerId && (p.hasCredential || p.hasApiKey)));
const baseline = ref(new Set<string>());
/** 一轮开始时记住历史身份，仅本轮新答案逐字显示，重新打开历史直接展示。 */
watch([() => props.session?.id, () => props.run?.id], () => {
  baseline.value = new Set(props.session?.messages.map(message => message.id) ?? []);
}, { immediate: true, flush: "sync" });
/** 当前计划在头部呈现；旧会话仅显示最后一条历史计划，避免整屏旧清单。 */
const visible = computed(() => {
  const all = props.session?.messages ?? [];
  const lastPlan = [...all].reverse().find(message => message.kind === "plan")?.id;
  const messages = all.filter(m => m.kind !== "running" && !(m.kind === "interrupted" && running.value)
    && (m.kind !== "plan" || (!props.session?.plan && m.id === lastPlan)));
  const run = props.run;
  const streamed = run?.text && run.sessionId === props.session?.id && !messages.some(message => run.textMessageId ? message.id === run.textMessageId : message.content === run.text)
    ? { id: run.textMessageId || `${run.id}-stream`, role: "assistant", kind: "text", content: run.text, data: null }
    : null;
  // 实时思考必须排在流式正文之前，和落盘后的历史顺序一致。
  const liveReasoning = run && showReasoning.value
    ? { id: run.reasoningMessageId || `${run.id}-reasoning`, role: "assistant", kind: "reasoning", content: run.reasoning, data: { live: true } }
    : null;
  return [...messages, ...(liveReasoning ? [liveReasoning] : []), ...(streamed ? [streamed] : [])];
});
/** 已停止的回答立即显示剩余文字，正常完成则播放完已收到的队列。 */
function animateMessage(message: AgentMessage): boolean {
  return message.role === "assistant" && message.kind === "text" && !!props.run && !["cancelled", "failed"].includes(props.run.state) && !baseline.value.has(message.id);
}
/** 历史附件只接受受支持的数据图片，不把任意消息字段作为外部链接加载。 */
function messageImages(message: AgentMessage): AgentImage[] {
  const images = message.data?.images;
  return Array.isArray(images) ? images.filter((image): image is AgentImage => !!image && typeof image.name === "string" && typeof image.dataBase64 === "string" && ["image/png", "image/jpeg", "image/webp"].includes(image.mimeType)) : [];
}
const reviews = computed(() => visible.value.filter(m => m.kind === "review"));
const activeReview = computed(() => selectedReview.value || reviews.value[reviews.value.length - 1]?.id);
const empty = computed(() => !visible.value.length && !running.value);
const suggestions = [
  { label: "一起学习笔记", text: "带我学习这些笔记，先了解我的目标" },
  { label: "开始今日复习", text: "开始今日复习" },
  { label: "整理知识库", text: "帮我整理笔记，先看看有哪些资料" },
];
/** 工具消息统一映射注册组件，正文与交互块各自承担展示职责。 */
const renderers: Record<string, typeof AgentAction> = { note: AgentAction, change: AgentAction, card: AgentAction, generate: AgentAction, memory: AgentAction, video: AgentVideo };
/** 只在用户仍靠近底部时跟随新消息，阅读历史不会被拉回。 */
function trackScroll(): void { const el = scroll.value; if (el) follow.value = el.scrollHeight - el.scrollTop - el.clientHeight < 100; }
/** 展开记忆编辑时获取最新保存内容，取消不会写入。 */
function editMemory(): void { memoryDraft.value = props.memory; memoryOpen.value = !memoryOpen.value; }
/** 窄窗口选择历史后收起抽屉，立即显示所选对话。 */
function selectHistory(id: string): void {
  emit("session", id);
  if (window.innerWidth < 768) historyOpen.value = false;
}
let scrollFrame: number | null = null;
/** 同帧所有进度合并一次布局读取；查看子详情时不拉动父历史。 */
function followOutput(): void {
  if (!follow.value || props.childDetail || scrollFrame !== null) return;
  scrollFrame = requestAnimationFrame(() => {
    scrollFrame = null;
    if (!follow.value || props.childDetail) return;
    const element = scroll.value;
    if (element) element.scrollTo?.({ top: element.scrollHeight });
  });
}
/** 取消未执行帧，避免卸载后的布局访问。 */
onBeforeUnmount(() => { if (scrollFrame !== null) cancelAnimationFrame(scrollFrame); });
watch(() => [props.run?.sequence, visible.value.length], followOutput);
/** 切换会话后恢复默认复习选择与新消息跟随。 */
watch(() => props.session?.id, () => { selectedReview.value = ""; follow.value = true; });
</script>
<template>
  <section class="agent-workspace relative flex h-full min-h-0 min-w-0 flex-1 flex-col bg-bg" aria-label="学习 Agent 工作区">
    <header class="flex min-h-14 shrink-0 flex-wrap items-center gap-1 px-3 py-1.5 sm:gap-2 sm:px-5">
      <span class="shrink-0 text-[15px] font-medium">学习 Agent</span>
      <span class="min-w-0 flex-1 truncate px-2 text-[12px] text-ink-2">{{ session?.title }}</span>
      <AgentHeaderChips class="shrink-0" :goal="autonomy.goal" :goal-phase="autonomy.goalPhase" :run-state="autonomy.runState"
        :waiting-reason="autonomy.waitingReason" :current-phase="run?.sessionId === session?.id ? run?.phase : undefined"
        :busy="running" :disabled="loading" :can-resume="ready && !sending && !readingImages"
        :children="children ?? []" :root-id="session?.id" :children-error="childrenError"
        @stop="emit('stop')" @resume-goal="emit('resumeGoal')" @refresh-children="emit('refreshChildren')"
        @open-child="emit('openChild', $event)" @interrupt-child="emit('interruptChild', $event)" @message-child="(id: string, message: string) => emit('messageChild', id, message)" />
      <button class="icon-btn" title="会话历史" aria-label="会话历史" :aria-expanded="historyOpen" @click="historyOpen = !historyOpen"><History :size="18" /></button>
      <button class="icon-btn" title="新建会话" aria-label="新建会话" :disabled="running || loading" @click="emit('newSession')"><Plus :size="18" /></button>
      <button class="icon-btn" title="长期记忆" aria-label="长期记忆" :aria-expanded="memoryOpen" @click="editMemory"><Brain :size="18" /></button>
      <button class="icon-btn" title="返回笔记" aria-label="返回笔记" @click="emit('close')"><X :size="18" /></button>
    </header>
    <div class="relative flex min-h-0 flex-1">
    <div class="flex min-h-0 min-w-0 flex-1 flex-col">
    <div v-if="memoryOpen" class="shrink-0 space-y-2 border-b border-hairline bg-bg-side p-4">
      <p class="text-[12px] text-ink-3">仅保存你明确指定的目标与偏好。清空并保存即可忘记。</p>
      <textarea v-model="memoryDraft" class="field-textarea" aria-label="长期学习记忆" maxlength="8000" />
      <button class="primary-btn" @click="emit('memory', memoryDraft); memoryOpen = false">保存记忆</button>
    </div>
    <div ref="outerScroll" class="flex min-h-0 flex-1 flex-col" :class="{ 'overflow-y-auto soft-scrollbar': empty }">
    <div ref="scroll" class="soft-scrollbar min-h-0" :class="empty ? 'shrink-0 pt-[clamp(24px,10vh,100px)]' : 'flex-1 overflow-y-auto'" @scroll="trackScroll">
      <div class="mx-auto w-full max-w-[792px] space-y-8 px-4 py-7">
        <!-- 当前计划是正文第一行折叠卡；历史 plan 消息在 visible 里被过滤，避免重复。 -->
        <AgentPlan v-if="autonomy.plan.steps.length" :plan="autonomy.plan" :evidence="autonomy.goal?.evidence" :criteria="autonomy.goal?.acceptanceCriteria" @open-child="emit('openChild', $event)" />
        <div v-if="empty" class="pb-1 text-center">
          <div class="mx-auto mb-5 grid size-12 place-items-center rounded-2xl bg-bg-side text-accent-strong"><Bot :size="26" /></div>
          <h1 class="text-[24px] font-medium tracking-tight sm:text-[28px]">今天想学点什么？</h1>
          <p class="mt-3 text-[14px] leading-6 text-ink-2">从你的笔记出发，理解、整理，再记牢。</p>
          <div class="mt-6 flex flex-wrap justify-center gap-2">
            <button v-for="suggestion in suggestions" :key="suggestion.text" class="rounded-full border border-hairline px-4 py-2 text-[13px] text-ink-2 transition-colors hover:bg-bg-side disabled:cursor-default disabled:opacity-40" :disabled="!ready || running || loading" @click="emit('send', suggestion.text)">{{ suggestion.label }}</button>
          </div>
        </div>
        <article v-for="message in visible" :key="message.id" class="min-w-0" :aria-label="['agent_message', 'goal_round'].includes(message.kind) ? '任务状态' : message.role === 'user' ? '你' : '学习 Agent'" :class="message.role === 'user' && message.kind !== 'agent_message' ? 'ml-auto w-fit max-w-[80%] rounded-[22px] bg-bg-side px-5 py-3' : 'w-full'">
          <AgentImages v-if="message.role === 'user'" :images="messageImages(message)" />
          <template v-if="message.kind === 'review'">
            <AgentReview v-if="message.id === activeReview && !running && !suspended" :flow="reviewFlow(message)" :ai-grading="aiGrading"
              @summary="emit('send', $event)" @generate="emit('send', '请帮助我从允许范围内的笔记生成学习卡片')" />
            <button v-else class="ghost-btn border border-hairline" :disabled="running" @click="selectedReview = message.id">{{ running ? '正在准备复习…' : '打开这轮复习' }}</button>
          </template>
          <AgentDrafts v-else-if="message.kind === 'drafts'" :message="message" :disabled="running" @adopt="(id, ids) => emit('adopt', id, ids)" />
          <AgentReasoning v-else-if="message.kind === 'reasoning'" :text="message.content" :running="running && message.data?.live === true" />
          <AgentSafeToolCalls v-else-if="['tool_calls', 'exchange'].includes(message.kind)" :message="message" />
          <AgentPlan v-else-if="message.kind === 'plan'" :message="message" :evidence="autonomy.goal?.evidence" :criteria="autonomy.goal?.acceptanceCriteria" @open-child="emit('openChild', $event)" />
          <AgentAutonomyNotice v-else-if="['agent_message', 'goal_round'].includes(message.kind)" :message="message" />
          <component :is="renderers[message.kind]" v-else-if="renderers[message.kind]" :message="message" :disabled="running" @action="(name: string, value: string) => emit('action', name, value)" />
          <AgentText v-else :text="message.content" :animate="animateMessage(message)" @progress="followOutput" @note="emit('action', 'note', $event)" />
        </article>
        <div v-if="running" class="space-y-3" role="status">
          <p class="flex items-center gap-2 text-[13px] text-ink-2"><span class="size-2 shrink-0 animate-pulse rounded-full bg-accent" />{{ run?.phase ?? '正在开始…' }}</p>
        </div>
      </div>
    </div>
    <footer class="shrink-0 px-4 pb-4 pt-2">
      <AgentComposer class="mx-auto w-full max-w-[760px]" :draft="draft" :selected-paths="selectedPaths" :notes="notes" :ready="ready" :running="running" :loading="loading" :error="error"
        :images="images" :reading-images="readingImages" @paste-images="emit('pasteImages', $event)" @remove-image="emit('removeImage', $event)"
        @send="emit('send', $event)" @stop="emit('stop')" @draft="emit('draft', $event)" @scope="emit('scope', $event)" @settings="emit('settings')" />
    </footer>
    </div>
    </div>
    <AgentChildDetail v-if="childDetail" v-bind="childDetail" :root-id="session?.id ?? ''" :children="children ?? []" :disabled="loading" :management-error="childrenError"
      @open-child="emit('openChild', $event)" @close-child="emit('closeChild')" @refresh-child="emit('refreshChild')"
      @interrupt-child="emit('interruptChild', $event)" @message-child="(id: string, message: string) => emit('messageChild', id, message)" />
    <AgentHistory v-if="historyOpen" :sessions="sessions" :active-id="session?.id" :busy="running || loading" @select="selectHistory" @delete="emit('deleteSession', $event)" @close="historyOpen = false" />
    <VirtualScrollbar :target="empty ? outerScroll : scroll" />
    </div>
  </section>
</template>

<style scoped>
.agent-workspace :deep(.icon-btn) { width: 36px; height: 36px; border-radius: 50%; }
.agent-workspace :deep(button:disabled) { cursor: default; }
header .icon-btn:disabled { opacity: 0.35; }
</style>
