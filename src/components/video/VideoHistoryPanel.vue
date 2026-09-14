<script setup lang="ts">
import type { VideoTaskHistory } from "../../domain/video";

/** 历史状态本地化，未知后端状态原样保留便于诊断。 */
const labels: Record<string, string> = { running: "进行中", completed: "已完成", failed: "失败", cancelled: "已停止", interrupted: "已中断" };

defineProps<{ items: VideoTaskHistory[]; error: string; busy: boolean }>();
/** 展开刷新与两个动作都交给父级，面板不持有任务状态。 */
defineEmits<{ refresh: []; restore: [item: VideoTaskHistory]; "open-note": [path: string] }>();
</script>

<template>
  <details class="rounded-2xl border border-hairline p-4 text-[12px]">
    <summary @click="$emit('refresh')">任务历史与恢复</summary>
    <p class="my-2 text-ink-3">仅显示你手动发起的任务，学习 Agent 自动发起的任务不会进入这里。恢复仅填入链接、分 P 与清晰度；确认后使用当前供应商与截图设置重新开始。</p>
    <p v-if="error" role="status">{{ error }}</p>
    <p v-if="!items.length" class="text-ink-3">暂无任务记录</p>
    <ul class="space-y-2">
      <li v-for="item in items" :key="item.taskId" class="flex flex-wrap items-center gap-2">
        <span class="min-w-0 flex-1 truncate" :title="item.error ?? item.title">{{ item.title || item.url }} · {{ labels[item.state] ?? item.state }}</span>
        <button v-if="item.notePath" class="ghost-btn" @click="$emit('open-note', item.notePath)">打开笔记</button>
        <button class="ghost-btn" :disabled="busy" @click="$emit('restore', item)">恢复参数</button>
      </li>
    </ul>
  </details>
</template>
