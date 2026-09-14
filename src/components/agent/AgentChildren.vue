<script setup lang="ts">
import { computed, ref, watch } from "vue";
import { RefreshCw, X } from "@lucide/vue";
import type { AgentChildInfo } from "../../domain/agent";
import { childTreeRows, type ChildTreeRow } from "./agentChildTree";
import AgentVirtualList from "./AgentVirtualList.vue";
import VirtualScrollbar from "../VirtualScrollbar.vue";

/**
 * 子代理下拉的内容：面板栏（摘要 + 刷新）+ 固定行高的虚拟列表 + 底部固定追加表单。
 * 追加表单从行内搬到面板底部，行高才能恒定为 52px，虚拟滚动才能按行高估算窗口；
 * 刷新、中断、追加消息仍以事件交还宿主：组件不调用 api、不消费 store。
 */
const props = defineProps<{ children: AgentChildInfo[]; rootId: string; error?: string; disabled?: boolean }>();
const emit = defineEmits<{ openChild: [id: string]; refreshChildren: []; interruptChild: [id: string]; messageChild: [id: string, message: string] }>();

/** 行高与视口高度是虚拟列表的估算基准：行内容必须两行以内，超出会被裁掉而不是撑高。 */
const ROW_HEIGHT = 52;
const VIEWPORT_HEIGHT = 320;
/** 缩进每层 16px 再加 12px 基础内缩；用 padding 而不是 margin，悬停底色才能铺满整行。 */
const INDENT_STEP = 16;
const INDENT_BASE = 12;
const STATUS_LABELS: Record<string, string> = { running: "运行中", idle: "空闲", ready: "可恢复" };
/** 长句只作为面板栏 title：正文里一句话够用，不再占一行。 */
const PANEL_HINT = "空闲或可恢复不代表任务已完成；接收也不等于执行完成。";
/** 孙级操作区只留短词，完整解释进 title，避免每行再堆一行说明。 */
const CHILD_HINT = "追加要求请通过直接父任务协调";

/** 虚拟列表实例：浮层要拿它内部的滚动容器，因此与列表同级渲染。 */
const list = ref<InstanceType<typeof AgentVirtualList> | null>(null);
const target = ref("");
const draft = ref("");
const validation = ref("");
/** 树层级只由持久化父关系决定，不能按返回列表顺序或 delegationDepth 伪造。 */
const rows = computed(() => childTreeRows(props.children, props.rootId));
/** 摘要只说事实：没有运行中任务时不显示“0 运行中”。 */
const summary = computed(() => {
  const running = rows.value.filter(row => row.child.status === "running").length;
  return running > 0 ? `${rows.value.length} 个子代理 · ${running} 运行中` : `${rows.value.length} 个子代理`;
});
/** 表单标题用描述而不是身份；描述缺失才回退 agentId，标题不会空掉。 */
const targetLabel = computed(() => props.children.find(child => child.agentId === target.value)?.description || target.value);

/** 虚拟列表只回传下标，这里还原成树行；越界时回退首行，渲染不会因下标漂移而崩。 */
function rowAt(index: number): ChildTreeRow { return rows.value[index] ?? rows.value[0]; }
/** 描述位置：关系异常优先显示父身份，坏记录可见但不伪造它的直属关系。 */
function rowLabel(row: ChildTreeRow): string {
  return row.detached ? `关系异常：${row.child.parentSessionId}` : row.child.description || row.child.agentId;
}
/** title 永远给全量描述：单行省略之后仍能悬停看全文。 */
function rowTooltip(row: ChildTreeRow): string {
  const description = row.child.description || row.child.agentId;
  return row.detached ? `${rowLabel(row)} · ${description}` : description;
}
/** 缩进量按真实父子层级换算，返回 CSS 长度字符串。 */
function rowIndent(depth: number): string { return `${depth * INDENT_STEP + INDENT_BASE}px`; }
/** 关闭表单并丢掉草稿：取消与发送成功后都走这里，选中状态只有一个出口。 */
function closeTarget(): void { target.value = ""; draft.value = ""; validation.value = ""; }
/** 点已选中的行收起表单；换目标即丢弃旧草稿，避免把要求错发到另一个子任务。 */
function toggleTarget(id: string): void {
  if (target.value === id) { closeTarget(); return; }
  target.value = id; draft.value = ""; validation.value = "";
}
/** 先本地拦空白、超字符与超字节（文案与宿主校验一致），再 emit；服务端仍会复验。 */
function sendMessage(): void {
  const message = draft.value.trim();
  if (!message) { validation.value = "请输入追加消息，不能只包含空白"; return; }
  if ([...draft.value].length > 16000) { validation.value = "追加消息不能超过 16000 字符"; return; }
  if (new TextEncoder().encode(draft.value).length > 16384) { validation.value = "追加消息的 UTF-8 大小不能超过 16 KiB"; return; }
  if (props.disabled || !props.children.some(child => child.agentId === target.value && child.parentSessionId === props.rootId)) return;
  emit("messageChild", target.value, message);
  closeTarget();
}
/** 根身份切换即丢弃局部追加草稿，避免把前一会话要求发到新任务。 */
watch(() => props.rootId, closeTarget);
</script>

<template>
  <div class="children-panel" aria-label="子 Agent 任务树">
    <!-- 面板栏：左侧摘要，右侧刷新；说明句只进 title，不占正文行。 -->
    <div class="panel-bar" :title="PANEL_HINT">
      <span class="summary">{{ summary }}</span>
      <button type="button" class="panel-action" aria-label="刷新子任务" title="刷新子任务" :disabled="disabled" @click="emit('refreshChildren')">
        <RefreshCw :size="13" />
      </button>
    </div>
    <p v-if="error" class="error" role="alert">{{ error }}</p>
    <p v-if="!rows.length" class="hint">暂无子任务。任务派生后会显示在这里。</p>
    <AgentVirtualList v-else ref="list" :count="rows.length" :item-height="ROW_HEIGHT" :viewport-height="VIEWPORT_HEIGHT" aria-label="子任务列表">
      <template #default="{ index }">
        <div class="row" :data-agent-id="rowAt(index).child.agentId" :data-depth="rowAt(index).depth" :style="{ paddingInlineStart: rowIndent(rowAt(index).depth) }">
          <div class="row-main">
            <span class="dot" :data-state="rowAt(index).child.status" aria-hidden="true" />
            <button type="button" class="description text-left hover:underline" :title="rowTooltip(rowAt(index))" @click="emit('openChild', rowAt(index).child.agentId)">{{ rowLabel(rowAt(index)) }}</button>
            <span class="state" :data-state="rowAt(index).child.status">{{ STATUS_LABELS[rowAt(index).child.status] ?? rowAt(index).child.status }}</span>
          </div>
          <!-- 操作行只在悬停或键盘聚焦时显现：行高恒定，10 个子任务也不会堆成文本墙。 -->
          <div class="row-actions">
            <button v-if="rowAt(index).child.status === 'running'" type="button" class="row-action" :disabled="disabled" title="仅中断该任务当前轮次，不停止后代" @click="emit('interruptChild', rowAt(index).child.agentId)">中断本轮</button>
            <button v-if="rowAt(index).child.parentSessionId === rootId" type="button" class="row-action" :disabled="disabled" :aria-expanded="target === rowAt(index).child.agentId" @click="toggleTarget(rowAt(index).child.agentId)">追加消息</button>
            <span v-else class="hint" :title="CHILD_HINT">经父级</span>
          </div>
        </div>
      </template>
    </AgentVirtualList>
    <VirtualScrollbar :target="list?.container ?? null" />
    <!-- 底部固定表单：不随列表滚动，追加入口不会滚出视口。 -->
    <form v-if="target" class="compose" @submit.prevent="sendMessage">
      <div class="compose-head">
        <span class="compose-title" :title="targetLabel">追加到：{{ targetLabel }}</span>
        <button type="button" class="panel-action" aria-label="关闭追加表单" title="关闭" @click="closeTarget"><X :size="13" /></button>
      </div>
      <textarea v-model="draft" class="field-textarea" aria-label="给子任务追加消息" rows="3" placeholder="补充要求或继续委派（最多 16000 字符 / UTF-8 16 KiB）" />
      <p v-if="validation" class="error" role="alert">{{ validation }}</p>
      <div class="compose-foot">
        <span class="hint">{{ [...draft].length }}/16000 · 接收不等于执行完成</span>
        <button type="submit" class="panel-action" :disabled="disabled">发送追加消息</button>
        <button type="button" class="panel-action" @click="closeTarget">取消</button>
      </div>
    </form>
  </div>
</template>

<style scoped>
/* 三段式纵向布局：面板栏、虚拟列表、底部表单；内边距由本组件提供，浮层只负责外框。 */
.children-panel { display: flex; flex-direction: column; gap: 6px; min-height: 0; padding: 8px 10px 10px; font-size: 12px; }
.panel-bar { display: flex; flex-shrink: 0; align-items: center; justify-content: space-between; gap: 8px; }
.summary { min-width: 0; overflow: hidden; white-space: nowrap; text-overflow: ellipsis; color: var(--color-ink); }
.panel-action { display: inline-flex; flex-shrink: 0; align-items: center; gap: 4px; height: 24px; padding: 0 6px; border-radius: 6px; font-size: 11px; color: var(--color-ink-2); }
.panel-action:hover { background: var(--color-bg-hover); color: var(--color-ink); }
.panel-action:disabled { opacity: 0.45; }
.hint { color: var(--color-ink-3); font-size: 11px; overflow-wrap: anywhere; }
.error { color: var(--color-danger); overflow-wrap: anywhere; }

/* 行内严格两行：状态行 18px + 操作行 18px + 内边距与 1px 分隔线 ≤ 52px。 */
.row { display: flex; flex-direction: column; justify-content: center; gap: 2px; height: 100%; padding: 6px 8px; overflow: hidden; border-bottom: 1px solid var(--color-hairline); }
.row:hover { background: var(--color-bg-hover); }
.row-main { display: flex; align-items: center; gap: 6px; height: 18px; min-width: 0; }
.dot { flex-shrink: 0; width: 6px; height: 6px; border-radius: 999px; background: var(--color-ink-3); }
.dot[data-state="running"] { background: var(--color-accent-strong); }
.dot[data-state="ready"] { background: var(--color-warning); }
/* min-width:0 是单行省略的前提：否则 flex 子项不肯收缩，省略号不会出现。 */
.description { flex: 1; min-width: 0; overflow: hidden; white-space: nowrap; text-overflow: ellipsis; color: var(--color-ink); }
/* 状态词不参与收缩，滚动条与省略号都挤不掉它。 */
.state { flex-shrink: 0; min-width: 4.5em; text-align: right; font-size: 11px; color: var(--color-ink-3); }
.state[data-state="running"] { color: var(--color-accent-strong); }
.state[data-state="ready"] { color: var(--color-warning); }
.row-actions { display: flex; align-items: center; gap: 6px; height: 18px; opacity: 0; transition: opacity 120ms; }
.row:hover .row-actions, .row:focus-within .row-actions { opacity: 1; }
.row-action { display: inline-flex; flex-shrink: 0; align-items: center; height: 18px; padding: 0 6px; border-radius: 5px; font-size: 11px; color: var(--color-ink-2); }
.row-action:hover { background: var(--color-bg-side); color: var(--color-ink); }

/* 底部表单固定不滚动：滚出视口就等于追加入口消失。 */
.compose { display: flex; flex-shrink: 0; flex-direction: column; gap: 6px; padding-top: 6px; border-top: 1px solid var(--color-hairline); }
.compose-head, .compose-foot { display: flex; align-items: center; gap: 8px; }
/* 标题单行省略，长描述不能把关闭按钮挤出面板。 */
.compose-title { flex: 1; min-width: 0; overflow: hidden; white-space: nowrap; text-overflow: ellipsis; font-weight: 600; }
.compose-foot .hint { margin-right: auto; min-width: 0; }
</style>
