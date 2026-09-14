<script setup lang="ts">
import { Check, LoaderCircle, Sparkles, X } from "@lucide/vue";
import { computed, ref } from "vue";
import type { AiSplitState } from "../services/aiSplitTypes";
import VirtualScrollbar from "./VirtualScrollbar.vue";

/** 草稿列表滚动容器：浮层滚动条与它同级。 */
const draftsScroll = ref<HTMLElement | null>(null);

const props = defineProps<{ state: AiSplitState; providerConfigured: boolean }>();
const emit = defineEmits<{
  close: []; adopt: []; start: []; stop: []; "open-settings": [];
  scope: [scope: "note" | "selection"]; count: [count: number];
  toggle: [draftId: string]; remove: [draftId: string];
}>();

const selectionText = computed(() => props.state.snapshot?.selection?.excerpt ?? "");
const phases = { preparing: "正在准备笔记…", planning: "正在规划考点清单…", generating: "正在生成记忆卡片…", lookup: "正在查询词典…", validating: "正在校验草稿…" };
const progressText = computed(() => `${props.state.stopping ? "正在停止生成…" : phases[props.state.phase]} 已生成 ${props.state.generatedCount} 张有效草稿`);
</script>

<template>
  <div class="overlay-backdrop flex items-start justify-center pt-20" @click.self="emit('close')">
    <div class="modal-panel w-full max-w-[480px] p-5">
      <header class="mb-4 flex items-center justify-between">
        <h2 class="flex items-center gap-2 text-[14px] font-semibold">
          <Sparkles :size="15" class="text-accent-strong" />AI 拆卡
        </h2>
        <button type="button" class="icon-btn" aria-label="关闭" :disabled="state.saving" @click="emit('close')"><X :size="15" /></button>
      </header>
      <p v-if="state.invalidReason" role="alert" class="mb-3 text-[11px] leading-5 text-danger">{{ state.invalidReason }}</p>
      <p v-for="warning in state.warnings" :key="warning" role="status" class="mb-2 text-[11px] leading-5 text-ink-2">{{ warning }}</p>

      <template v-if="state.step === 'scope'">
        <div v-if="!providerConfigured" class="mb-3 flex items-center justify-between gap-3 rounded-lg border border-warning/40 bg-warning/8 px-3 py-2.5">
          <p class="text-[11px] leading-5 text-ink-2">当前 AI 供应商尚未配置可用的模型与凭据，请先在设置中配置。</p>
          <button type="button" class="ghost-btn shrink-0 border border-hairline" @click="emit('open-settings')">打开设置</button>
        </div>
        <div class="mb-3 grid grid-cols-2 gap-2">
          <button type="button" class="rounded-lg border p-3 text-left transition" :class="state.scope === 'note' ? 'border-accent bg-bg-active/40' : 'border-hairline hover:border-accent/50'" @click="emit('scope', 'note')">
            <p class="text-[12px] font-medium">整篇笔记</p>
            <p class="mt-0.5 text-[10px] text-ink-3">通读全文提炼知识点</p>
          </button>
          <button type="button" class="rounded-lg border p-3 text-left transition" :class="state.scope === 'selection' ? 'border-accent bg-bg-active/40' : 'border-hairline hover:border-accent/50'" :disabled="!selectionText" @click="emit('scope', 'selection')">
            <p class="text-[12px] font-medium">选中段落</p>
            <p class="mt-0.5 text-[10px] text-ink-3">{{ selectionText ? selectionText.slice(0, 18) + "…" : "先在正文中选中一段话" }}</p>
          </button>
        </div>
        <div class="mb-4 flex items-center gap-1.5">
          <button v-for="option in [{ count: -1, label: '自动' }, { count: 5, label: '最多 5 张' }, { count: 10, label: '最多 10 张' }]" :key="option.count" type="button" class="rounded-full border px-3 py-1 text-[11px] transition" :class="state.requestedCount === option.count ? 'border-accent bg-bg-active/40 text-accent-strong' : 'border-hairline text-ink-2 hover:border-accent/50'" @click="emit('count', option.count)">
            {{ option.label }}
          </button>
        </div>
        <footer class="flex justify-end gap-2">
          <button type="button" class="ghost-btn" @click="emit('close')">取消</button>
          <button type="button" class="primary-btn" :disabled="!providerConfigured || Boolean(state.invalidReason)" @click="emit('start')"><Sparkles :size="14" />开始拆卡</button>
        </footer>
      </template>

      <template v-else-if="state.step === 'running'">
        <div class="flex flex-col items-center py-8">
          <LoaderCircle :size="22" class="animate-spin text-accent-strong" />
          <p role="status" class="stream-caret mt-4 min-h-5 text-center text-[12px] text-ink-2">{{ progressText }}</p>
        </div>
        <footer class="flex justify-end">
          <button type="button" class="ghost-btn" :disabled="state.stopping" @click="emit('stop')">{{ state.stopping ? '正在停止…' : '停止生成' }}</button>
        </footer>
      </template>

      <template v-else>
        <p class="mb-3 text-[11px] text-ink-3">{{ state.drafts.length ? `生成 ${state.drafts.length} 张草稿，勾选要保留的卡片：` : '没有生成可采纳的卡片。' }}</p>
        <ul ref="draftsScroll" class="soft-scrollbar max-h-[46vh] space-y-2 overflow-y-auto pr-1">
          <li v-for="draft in state.drafts" :key="draft.draftId" class="rounded-lg border border-hairline bg-bg-paper p-3">
            <div class="flex items-start gap-2.5">
              <button type="button" class="mt-0.5 grid size-4 shrink-0 place-items-center rounded border transition" :class="state.accepted.has(draft.draftId) ? 'border-accent bg-accent text-white' : 'border-ink-3'" aria-label="保留这张卡片" :aria-pressed="state.accepted.has(draft.draftId)" :disabled="state.saving" @click="emit('toggle', draft.draftId)">
                <Check v-if="state.accepted.has(draft.draftId)" :size="11" />
              </button>
              <div class="min-w-0 flex-1">
                <p class="text-[12px] leading-5 font-medium">{{ draft.fields.front }}</p>
                <p class="mt-0.5 line-clamp-2 text-[11px] leading-4 text-ink-3">{{ draft.fields.back }}</p>
                <p class="mt-1 text-[10px] text-ink-3">来源：{{ (draft.fields.source || draft.source?.excerpt || '未提供原文摘录').slice(0, 30) }}</p>
              </div>
              <button type="button" class="icon-btn !size-6 shrink-0" title="移除这张草稿" :disabled="state.saving" @click="emit('remove', draft.draftId)"><X :size="12" /></button>
            </div>
          </li>
        </ul>
        <VirtualScrollbar :target="draftsScroll" />
        <footer class="mt-4 flex justify-end gap-2">
          <button type="button" class="ghost-btn" :disabled="state.saving" @click="emit('close')">放弃全部</button>
          <button type="button" class="primary-btn" :disabled="state.accepted.size === 0 || state.saving || Boolean(state.invalidReason)" @click="emit('adopt')">
            <Check :size="14" />{{ state.saving ? '正在采纳…' : `采纳 ${state.accepted.size} 张` }}
          </button>
        </footer>
      </template>
    </div>
  </div>
</template>
