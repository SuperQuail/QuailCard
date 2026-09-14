<script setup lang="ts">
import { computed, onBeforeUnmount, onMounted, ref } from "vue";
import { ChevronDown, X } from "@lucide/vue";
import type { AgentChildInfo, AgentRunState, Goal } from "../../domain/agent";
import { goalPhaseHint, goalPhaseWord } from "./agentGoalPhase";
import AgentChildren from "./AgentChildren.vue";
import AgentGoalPanel from "./AgentGoalPanel.vue";
import VirtualScrollbar from "../VirtualScrollbar.vue";

/** 两个浮层各自可能超高滚动，各配一个与它同级的浮层滚动条。 */
const goalScroll = ref<HTMLElement | null>(null);
const childrenScroll = ref<HTMLElement | null>(null);

/**
 * 会话头部 chip 排：把目标与子代理压成一排 28px 的小按钮，
 * 内容一律进下拉浮层，不占聊天正文；只接收展示事实，缺省即不渲染，
 * 所有动作都以事件交还宿主，组件不调用 api、不消费 store。
 */
const props = defineProps<{
  goal?: Goal | null; goalPhase?: string | null; runState?: AgentRunState | null;
  waitingReason?: string | null; busy?: boolean; disabled?: boolean; canResume?: boolean;
  children?: AgentChildInfo[]; rootId?: string; childrenError?: string; currentPhase?: string;
}>();
const emit = defineEmits<{
  openChild: [id: string]; stop: []; resumeGoal: []; refreshChildren: []; interruptChild: [id: string]; messageChild: [id: string, message: string];
}>();
const goalPanelId = "agent-goal-popover";
const childrenPanelId = "agent-children-popover";
/** 同时只允许一个下拉展开，头部不会叠出多层浮层。 */
const open = ref<"goal" | "children" | "">("");
const root = ref<HTMLElement | null>(null);
const childCount = computed(() => props.children?.length ?? 0);
/** 没有目标不占位置；子树首次读取失败时保留入口以便查看错误并重试。 */
const showGoal = computed(() => !!props.goal);
const showChildren = computed(() => !!props.rootId && (childCount.value > 0 || !!props.childrenError));
/** chip 文本必须是极短状态词，长句只能进 title 或展开面板。 */
const goalWord = computed(() => goalPhaseWord(props.goalPhase, props.runState, !!props.busy, props.goal?.phase));
const goalHint = computed(() => goalPhaseHint(props.goalPhase, props.runState, !!props.busy) || props.goal?.objective || "");
/** 点同一个 chip 收起，点另一个直接切换，不需要先关闭。 */
function toggle(name: "goal" | "children"): void {
  open.value = open.value === name ? "" : name;
}
/** 打开独立详情时收起浮层，但不触发主会话选择。 */
function openChild(id: string): void { open.value = ""; emit("openChild", id); }
/** 点击浮层内部不关闭；点击页面其他位置或 Esc 收起，避免久留遮挡正文。 */
function onPointerDown(event: MouseEvent): void {
  if (!open.value) return;
  const node = event.target;
  if (root.value && node instanceof Node && root.value.contains(node)) return;
  open.value = "";
}
/** Esc 关闭是浮层的基本预期，键盘用户无需去够关闭按钮。 */
function onKeyDown(event: KeyboardEvent): void {
  if (event.key === "Escape") open.value = "";
}
onMounted(() => {
  document.addEventListener("mousedown", onPointerDown);
  document.addEventListener("keydown", onKeyDown);
});
// 卸载必须摘掉 document 监听，否则会话切换后会留下引用与幽灵关闭行为。
onBeforeUnmount(() => {
  document.removeEventListener("mousedown", onPointerDown);
  document.removeEventListener("keydown", onKeyDown);
});
</script>
<template>
  <div ref="root" class="agent-chips" aria-label="自主任务状态">
    <div v-if="showGoal" class="chip-wrap">
      <button type="button" class="ghost-btn chip" data-chip="goal" :title="goalHint" :aria-expanded="open === 'goal'"
        :aria-controls="open === 'goal' ? goalPanelId : undefined" @click="toggle('goal')">
        <span class="chip-label">目标 · {{ goalWord }}</span>
        <ChevronDown class="chevron" :data-open="open === 'goal' || undefined" :size="13" />
      </button>
      <div v-if="open === 'goal'" :id="goalPanelId" ref="goalScroll" class="popover soft-scrollbar" role="dialog" aria-label="目标">
        <div class="pop-head">
          <span class="pop-title">目标 · {{ goalWord }}</span>
          <button type="button" class="pop-icon" aria-label="关闭目标面板" @click="open = ''"><X :size="14" /></button>
        </div>
        <p v-if="goal?.objective" class="pop-objective">{{ goal.objective }}</p>
        <AgentGoalPanel :goal="goal ?? null" :goal-phase="goalPhase ?? null" :run-state="runState ?? null"
          :waiting-reason="waitingReason ?? null" :current-phase="currentPhase" :busy="!!busy" :disabled="disabled"
          :can-resume="canResume" @stop="emit('stop')" @resume-goal="emit('resumeGoal')" />
      </div>
      <VirtualScrollbar :target="goalScroll" />
    </div>
    <div v-if="showChildren" class="chip-wrap">
      <button type="button" class="ghost-btn chip" data-chip="children" title="子代理任务树"
        :aria-expanded="open === 'children'" :aria-controls="open === 'children' ? childrenPanelId : undefined"
        @click="toggle('children')">
        <span class="chip-label">{{ childCount }} 个子代理</span>
        <ChevronDown class="chevron" :data-open="open === 'children' || undefined" :size="13" />
      </button>
      <div v-if="open === 'children'" :id="childrenPanelId" ref="childrenScroll" class="popover popover-list soft-scrollbar" role="dialog" aria-label="子代理">
        <div class="pop-head">
          <span class="pop-title">子代理 {{ childCount }}</span>
          <button type="button" class="pop-icon" aria-label="关闭子代理面板" @click="open = ''"><X :size="14" /></button>
        </div>
        <AgentChildren :root-id="rootId ?? ''" :children="children ?? []" :error="childrenError" :disabled="disabled"
          @open-child="openChild" @refresh-children="emit('refreshChildren')" @interrupt-child="emit('interruptChild', $event)"
          @message-child="(id: string, message: string) => emit('messageChild', id, message)" />
      </div>
      <VirtualScrollbar :target="childrenScroll" />
    </div>
  </div>
</template>
<style scoped>
.agent-chips { display: flex; align-items: center; gap: 4px; min-width: 0; }
.chip-wrap { position: relative; min-width: 0; }
.chip { height: 28px; max-width: 100%; padding: 0 8px; border-radius: 6px; background: var(--color-bg-side); font-size: 12px; color: var(--color-ink-2); }
.chip:hover { background: var(--color-bg-active); color: var(--color-ink); }
.chip[aria-expanded="true"] { background: var(--color-bg-active); color: var(--color-accent-strong); }
.chip-label { min-width: 0; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
.chevron { flex-shrink: 0; color: var(--color-ink-3); transition: transform 120ms; }
.chevron[data-open] { transform: rotate(180deg); }
.popover { position: absolute; top: calc(100% + 6px); right: 0; z-index: 40; width: min(420px, calc(100vw - 32px)); max-height: 60vh; overflow-y: auto; padding: 8px 10px 10px; border: 1px solid var(--color-hairline); border-radius: 8px; background: var(--color-bg-paper); box-shadow: 0 10px 28px rgb(0 0 0 / 16%); font-size: 12px; }
/* 子代理浮层：整块不再自己滚动，高度交给内部虚拟列表，原生滚动条不会压住右侧状态词。
   必须声明在 .popover 之后：同优先级下靠顺序覆盖它的 width / max-height / overflow / padding。 */
.popover-list { display: flex; flex-direction: column; width: min(480px, calc(100vw - 32px)); max-height: 60vh; overflow: hidden; padding: 0; }
/* 外框 padding 归零后，标题行自己补回内边距；下方交给 AgentChildren 提供。 */
.popover-list .pop-head { padding: 8px 10px 0; }
.pop-head { display: flex; align-items: center; gap: 8px; padding-bottom: 6px; }
.pop-title { font-size: 12px; font-weight: 600; }
.pop-icon { display: grid; place-items: center; width: 24px; height: 24px; margin-left: auto; border-radius: 6px; color: var(--color-ink-2); }
.pop-icon:hover { background: var(--color-bg-hover); color: var(--color-ink); }
.pop-objective { padding-bottom: 8px; font-size: 13px; line-height: 1.7; color: var(--color-ink); white-space: pre-wrap; overflow-wrap: anywhere; }
</style>
