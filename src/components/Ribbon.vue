<script setup lang="ts">
import { Bot, Brain, Moon, PanelLeftClose, PanelLeftOpen, PenLine, Play, Search, Settings, Sun } from "@lucide/vue";

defineProps<{
  treeOpen: boolean;
  dueCount: number;
  dark: boolean;
  agentOpen?: boolean;
  reviewOpen?: boolean;
  videoOpen?: boolean;
}>();

const emit = defineEmits<{
  "toggle-tree": [];
  "open-palette": [];
  "open-capture": [];
  "open-review": [];
  "toggle-theme": [];
  "open-settings": [];
  "open-agent": [];
  "open-video": [];
}>();
</script>

<template>
  <!-- 左侧丝带：顶部是文件树开关，其余为全局入口 -->
  <nav class="flex w-[52px] shrink-0 flex-col items-center gap-2 border-r border-hairline bg-bg-side py-3" aria-label="工作区丝带">
    <button type="button" class="icon-btn mb-1.5" :class="{ active: treeOpen }" :title="treeOpen ? '收起文件树' : '展开文件树'" @click="emit('toggle-tree')">
      <PanelLeftClose v-if="treeOpen" :size="17" :stroke-width="1.8" />
      <PanelLeftOpen v-else :size="17" :stroke-width="1.8" />
    </button>

    <button type="button" class="icon-btn" title="搜索（Ctrl+K）" @click="emit('open-palette')">
      <Search :size="17" :stroke-width="1.8" />
    </button>
    <button type="button" class="icon-btn" title="快速捕获（Ctrl+N）" @click="emit('open-capture')">
      <PenLine :size="17" :stroke-width="1.8" />
    </button>

    <div class="my-2 h-px w-7 bg-hairline" />

    <button type="button" class="icon-btn" :class="{ active: agentOpen }" title="学习 Agent" aria-label="打开学习 Agent" :aria-pressed="agentOpen" @click="emit('open-agent')">
      <Bot :size="18" :stroke-width="1.8" />
    </button>

    <button type="button" class="icon-btn" :class="{ active: videoOpen }" title="视频转笔记" aria-label="打开视频转笔记" :aria-pressed="videoOpen" @click="emit('open-video')">
      <Play :size="17" :stroke-width="1.8" />
    </button>

    <button type="button" class="icon-btn relative" :class="{ active: reviewOpen }" title="复习工作区" aria-label="打开复习工作区" :aria-pressed="reviewOpen" @click="emit('open-review')">
      <Brain :size="17" :stroke-width="1.8" />
      <span
        v-if="dueCount > 0"
        class="absolute -top-0.5 -right-0.5 grid min-w-4 place-items-center rounded-full bg-accent px-1 text-[9px] font-semibold leading-4 text-white"
      >{{ dueCount }}</span>
    </button>

    <div class="mt-auto flex shrink-0 flex-col items-center gap-2 pt-3">
      <button type="button" class="icon-btn" title="切换主题" @click="emit('toggle-theme')">
        <Sun v-if="dark" :size="17" :stroke-width="1.8" />
        <Moon v-else :size="17" :stroke-width="1.8" />
      </button>
      <button type="button" class="icon-btn" title="设置" @click="emit('open-settings')">
        <Settings :size="17" :stroke-width="1.8" />
      </button>
    </div>
  </nav>
</template>
