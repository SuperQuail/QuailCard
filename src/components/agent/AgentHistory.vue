<script setup lang="ts">
import { computed, ref, watch } from "vue";
import { MessageSquare, Search, Trash2, X } from "@lucide/vue";
import type { AgentSession } from "../../domain/agent";
import VirtualScrollbar from "../VirtualScrollbar.vue";

/** 会话列表滚动容器：浮层滚动条与它同级。 */
const historyScroll = ref<HTMLElement | null>(null);

const props = defineProps<{ sessions: AgentSession[]; activeId?: string; busy: boolean }>();
const emit = defineEmits<{ select: [id: string]; delete: [id: string]; close: [] }>();
const query = ref("");
const pending = ref("");
const matches = computed(() => [...props.sessions].sort((a, b) => b.updatedAt - a.updatedAt).filter(item => item.title.toLowerCase().includes(query.value.trim().toLowerCase())));
/** 时间用于区分同名会话，无效时间不暴露格式异常。 */
function date(value: number): string { return value ? new Date(value * 1000).toLocaleString("zh-CN", { month: "short", day: "numeric", hour: "2-digit", minute: "2-digit" }) : ""; }
/** 删除成功或切换会话后清除行内确认。 */
watch(() => [props.sessions, props.activeId], () => { pending.value = ""; });
</script>

<template>
  <aside class="agent-history flex min-h-0 shrink-0 flex-col border-l border-hairline bg-bg-side" aria-label="会话历史管理" @keydown.esc.stop="emit('close')">
    <div class="flex h-14 shrink-0 items-center justify-between px-4">
      <h2 class="text-[14px] font-medium">历史记录 <span class="ml-1 text-[12px] text-ink-2">{{ sessions.length }}</span></h2>
      <button class="icon-btn" aria-label="关闭历史记录" @click="emit('close')"><X :size="17" /></button>
    </div>
    <div class="relative mx-3 mb-3">
      <Search :size="15" class="pointer-events-none absolute left-3 top-2.5 text-ink-2" />
      <input v-model="query" class="field-input !h-9 !rounded-xl !pl-9" aria-label="搜索历史记录" placeholder="搜索对话…" />
    </div>
    <div ref="historyScroll" class="soft-scrollbar min-h-0 flex-1 space-y-1 overflow-y-auto px-2 pb-3">
      <div v-for="item in matches" :key="item.id" class="rounded-xl p-1" :class="item.id === activeId ? 'bg-bg-active' : 'hover:bg-bg-hover'">
        <div class="flex items-center gap-1">
          <button class="flex min-w-0 flex-1 items-center gap-2 rounded-lg px-2 py-2 text-left disabled:opacity-50" :aria-current="item.id === activeId ? 'true' : undefined" :disabled="busy" @click="emit('select', item.id)">
            <MessageSquare :size="15" class="shrink-0 text-ink-2" />
            <span class="min-w-0"><span class="block truncate text-[13px]" :title="item.title">{{ item.title }}</span><span class="mt-1 block text-[11px] text-ink-2">{{ date(item.updatedAt) }}</span></span>
          </button>
          <button class="icon-btn shrink-0 hover:!text-danger disabled:opacity-40" :aria-label="`删除会话：${item.title}`" title="删除会话" :disabled="busy" @click="pending = pending === item.id ? '' : item.id"><Trash2 :size="15" /></button>
        </div>
        <div v-if="pending === item.id" class="px-2 pb-2 text-[12px]">
          <p class="mb-2 leading-5 text-ink-2">删除这条对话？已生成的笔记和卡片会保留。</p>
          <div class="flex justify-end gap-2"><button class="ghost-btn" :disabled="busy" @click="pending = ''">取消</button><button class="ghost-btn !text-danger" :disabled="busy" @click="emit('delete', item.id)">确认删除</button></div>
        </div>
      </div>
      <p v-if="!matches.length" class="px-3 py-8 text-center text-[13px] text-ink-2">{{ sessions.length ? '没有匹配的对话' : '暂无历史记录' }}</p>
    </div>
    <VirtualScrollbar :target="historyScroll" />
    <p v-if="busy" class="px-4 py-3 text-[12px] text-ink-2" role="status">请等待当前操作完成后管理历史记录</p>
  </aside>
</template>

<style scoped>
.agent-history { width: 280px; }
@media (max-width: 767px) {
  .agent-history { position: absolute; inset: 0 0 0 auto; z-index: 20; width: min(320px, 100%); box-shadow: -8px 0 24px rgb(0 0 0 / 8%); }
}
</style>
