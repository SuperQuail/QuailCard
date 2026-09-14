<script setup lang="ts">
import { computed, ref } from "vue";
import { Link, Play, Radar, RotateCw, Zap } from "@lucide/vue";
import type { VideoMode, VideoProbe } from "../../domain/video";
import { formatBytes, formatDuration } from "../../domain/video";
import VirtualScrollbar from "../VirtualScrollbar.vue";

/** 分 P 列表滚动容器：浮层滚动条与它同级。 */
const pagesScroll = ref<HTMLElement | null>(null);

const props = defineProps<{
  url: string;
  probe: VideoProbe | null;
  loggedIn: boolean;
  forceTranscribe: boolean;
  quality: number;
  pages: number[];
  screenshots: boolean;
  mode: VideoMode;
  probing: boolean;
  starting: boolean;
  running: boolean;
}>();
const emit = defineEmits<{
  "update:url": [value: string];
  probe: [];
  "update:quality": [value: number];
  "toggle-page": [page: number];
  "update:screenshots": [value: boolean];
  "update:forceTranscribe": [value: boolean];
  "update:mode": [value: VideoMode];
  start: [];
}>();

/** 仅允许提交可用清晰度和非空的有效分 P 选择，避免发送失效选项。 */
const canStart = computed(() => Boolean(
  props.probe?.qualities.some(item => item.qn === props.quality && item.available)
  && props.pages.length > 0 && selectedPages.value.length === new Set(props.pages).size
) && !props.starting && !props.running && !props.probing);
/** 以解析结果中的分 P 为准汇总，重复选择不会重复计算。 */
const selectedPages = computed(() => props.probe?.pages.filter(page => props.pages.includes(page.page)) ?? []);
/** 汇总所选分 P 的时长，不把估算基准时长误当总时长。 */
const selectedDuration = computed(() => selectedPages.value.reduce((total, page) => total + page.duration, 0));
/** 清晰度展示：不可用档位保留在列表中但禁用，说明原因。 */
const qualities = computed(() => props.probe?.qualities ?? []);
/** 多分 P 时才展示分 P 选择，单 P 视频保持界面简洁。 */
const multiplePages = computed(() => (props.probe?.pages.length ?? 0) > 1);
/** 字幕模式只输出时间轴字幕稿，后端会忽略截图开关，因此界面同步禁用截图选项。 */
const transcriptMode = computed(() => props.mode === "transcript");

/** 不承诺登录即可解锁；估算体积按解析基准时长缩放到所选分 P。 */
function qualityHint(available: boolean, requiresVip: boolean, estimatedBytes: number, estimatedDuration?: number, reason?: string): string {
  if (!available && reason?.trim()) return reason;
  if (!available) return requiresVip ? "需要大会员"
    : props.loggedIn ? "当前账号或视频不支持" : "登录后重试（仍可能受账号或视频限制）";
  const baseDuration = estimatedDuration ?? props.probe?.duration ?? 0;
  const bytes = baseDuration > 0 ? estimatedBytes * selectedDuration.value / baseDuration : 0;
  return bytes > 0 && Number.isFinite(bytes) ? "约 " + formatBytes(bytes) : "估算不可用";
}
</script>

<template>
  <section class="space-y-3 rounded-2xl border border-hairline bg-bg-paper p-4">
    <div class="flex items-center gap-2 text-[13px] text-ink-2">
      <Link :size="15" :stroke-width="1.8" />
      <span>粘贴 B 站视频链接（支持分享文本、b23.tv 短链与裸 BV 号）</span>
    </div>
    <div class="flex flex-wrap items-center gap-2">
      <input
        class="field-input min-w-0 flex-1"
        :value="url"
        aria-label="视频链接"
        placeholder="https://www.bilibili.com/video/BV..."
        @input="emit('update:url', ($event.target as HTMLInputElement).value)"
        @keydown.enter.prevent="emit('probe')"
      />
      <button class="ghost-btn border border-hairline" :disabled="probing || !url.trim()" @click="emit('probe')">
        <Radar :size="15" :stroke-width="1.8" />{{ probing ? "解析中…" : "解析" }}
      </button>
    </div>

    <div v-if="probe" class="space-y-3 border-t border-hairline pt-3">
      <div class="min-w-0">
        <p class="truncate text-[14px] font-medium" :title="probe.title">{{ probe.title }}</p>
        <p class="mt-1 text-[12px] text-ink-2">
          UP 主：{{ probe.owner || "未知" }} · 已选 {{ selectedPages.length }} P · 总时长：{{ formatDuration(selectedDuration) }} ·
          {{ loggedIn ? "已登录" : "未登录" }}
        </p>
      </div>

      <fieldset class="space-y-1">
        <legend class="text-[12px] text-ink-2">输出模式</legend>
        <label class="flex items-center gap-2 text-[12px] text-ink-2">
          <input
            type="radio"
            name="video-mode"
            value="note"
            :checked="mode === 'note'"
            @change="emit('update:mode', 'note')"
          />
          笔记：提取并重建信息（无时间轴，可配关键画面）
        </label>
        <label class="flex items-center gap-2 text-[12px] text-ink-2">
          <input
            type="radio"
            name="video-mode"
            value="transcript"
            :checked="mode === 'transcript'"
            @change="emit('update:mode', 'transcript')"
          />
          字幕：保留时间轴的字幕稿（不生成笔记）
        </label>
      </fieldset>

      <div class="grid gap-2 sm:grid-cols-[1fr_auto] sm:items-center">
        <label class="flex items-center gap-2 text-[12px] text-ink-2">
          清晰度
          <select
            class="field-input min-w-0 flex-1"
            :value="quality"
            aria-label="视频清晰度"
            @change="emit('update:quality', Number(($event.target as HTMLSelectElement).value))"
          >
            <option v-for="item in qualities" :key="item.qn" :value="item.qn" :disabled="!item.available">
              {{ item.label }} · {{ qualityHint(item.available, item.requiresVip, item.estimatedBytes, item.estimatedDuration, item.unavailableReason) }}
            </option>
          </select>
        </label>
        <label class="flex items-center gap-2 text-[12px] text-ink-2">
          <input
            type="checkbox"
            :checked="screenshots"
            :disabled="transcriptMode"
            @change="emit('update:screenshots', ($event.target as HTMLInputElement).checked)"
          />
          生成关键画面截图
          <span v-if="transcriptMode" class="text-ink-3">字幕模式不使用截图</span>
        </label>
        <label class="flex items-center gap-2 text-[12px] text-ink-2">
          <input
            type="checkbox"
            :checked="forceTranscribe"
            @change="emit('update:forceTranscribe', ($event.target as HTMLInputElement).checked)"
          />
          强制本地转写（忽略现有字幕）
        </label>
      </div>

      <p v-if="!loggedIn" class="flex items-center gap-1 text-[12px] text-ink-2">
        <Zap :size="13" />未登录时清晰度上限通常为 480P；登录成功会自动重新解析并解锁更高档位。
      </p>

      <div v-if="multiplePages" class="space-y-1">
        <p class="text-[12px] text-ink-2">选择要处理的分 P（可多选）</p>
        <div ref="pagesScroll" class="soft-scrollbar max-h-32 space-y-1 overflow-y-auto">
          <label v-for="page in probe.pages" :key="page.page" class="flex items-center gap-2 rounded-lg px-2 py-1 text-[12px] hover:bg-bg-hover">
            <input type="checkbox" :checked="pages.includes(page.page)" @change="emit('toggle-page', page.page)" />
            <span class="min-w-0 truncate">P{{ page.page }} {{ page.title || "未命名" }}</span>
            <span class="shrink-0 text-ink-3">{{ formatDuration(page.duration) }}</span>
          </label>
        </div>
        <VirtualScrollbar :target="pagesScroll" />
      </div>

      <button class="primary-btn" :disabled="!canStart" @click="emit('start')">
        <RotateCw v-if="starting" :size="15" class="animate-spin" />
        <Play v-else :size="15" />
        {{ starting ? "正在开始…" : running ? "任务进行中" : "开始生成笔记" }}
      </button>
      <p v-if="probe.qualities.every(item => !item.available)" class="flex items-center gap-1 text-[12px] text-ink-2">
        <Zap :size="13" />当前清晰度都不可用，可能受账号权限或视频限制；请检查权限后重新解析。
      </p>
    </div>
  </section>
</template>
