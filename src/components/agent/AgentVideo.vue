<script setup lang="ts">
import { computed, ref } from "vue";
import { FileText, Film, ScanLine } from "@lucide/vue";
import type { AgentMessage } from "../../domain/agent";
import { transcriptLabel } from "../../domain/video";
import VirtualScrollbar from "../VirtualScrollbar.vue";

/** 转录预览滚动容器：浮层滚动条与它同级。 */
const previewScroll = ref<HTMLElement | null>(null);

const props = defineProps<{ message: AgentMessage; disabled?: boolean }>();
const emit = defineEmits<{ action: [name: string, value: string] }>();

/** 工具结果数据形状由后端注册表决定，这里只读取展示必需字段。 */
const data = computed(() => (props.message.data ?? {}) as Record<string, unknown>);
/** 笔记路径存在时给出打开入口。 */
const notePath = computed(() => String(data.value.notePath ?? ""));
/** 是取字结果还是成文结果。 */
const isTranscript = computed(() => data.value.output === "transcript");
/** 转录摘要，取字结果里用于让用户直接看到内容。 */
const preview = computed(() => String(data.value.preview ?? ""));
/** 段落数与截图数。 */
const stats = computed(() => {
  const segments = Number(data.value.segments ?? 0);
  const shots = Number(data.value.shots ?? 0);
  const parts: string[] = [];
  if (segments > 0) parts.push(segments + " 段语音");
  if (shots > 0) parts.push(shots + " 张截图");
  return parts.join(" · ");
});
</script>

<template>
  <section class="space-y-2 rounded-2xl border border-hairline bg-bg-side px-4 py-3">
    <header class="flex items-center gap-2 text-[13px] font-medium">
      <Film :size="15" :stroke-width="1.8" />
      <span>{{ isTranscript ? "视频转录" : "视频笔记" }}</span>
      <span class="text-[12px] font-normal text-ink-2">
        {{ transcriptLabel(String(data.transcriptSource ?? "")) }}
        <template v-if="stats"> · {{ stats }}</template>
      </span>
    </header>

    <VirtualScrollbar v-if="preview" :target="previewScroll" />
    <pre v-if="preview" ref="previewScroll" class="soft-scrollbar max-h-40 overflow-auto text-[12px] leading-5 text-ink-2 whitespace-pre-wrap">{{ preview }}</pre>

    <div class="flex flex-wrap gap-2">
      <button v-if="notePath" class="primary-btn" :disabled="disabled" @click="emit('action', 'note', notePath)">
        <FileText :size="15" />打开笔记
      </button>
      <p v-else class="flex items-center gap-1 text-[12px] text-ink-2">
        <ScanLine :size="13" />继续对话可以让 Agent 基于这份转录生成笔记。
      </p>
    </div>
  </section>
</template>
