<!----------------------------------------------------------------
 外观设置：主题与正文字号。全部动作上抛，组件不自持业务。
---------------------------------------------------------------->
<script setup lang="ts">
import type { FontSizeId, ThemeId } from "../../domain/types";

defineProps<{
  theme: ThemeId;
  fontSize: FontSizeId;
}>();

const emit = defineEmits<{
  "update-theme": [theme: ThemeId];
  "update-font-size": [size: FontSizeId];
}>();

/** 字号档位选项（紧凑/标准/舒适/大）。 */
const fontSizeOptions: Array<{ id: FontSizeId; label: string }> = [
  { id: "compact", label: "紧凑" },
  { id: "standard", label: "标准" },
  { id: "comfortable", label: "舒适" },
  { id: "large", label: "大" },
];
</script>

<template>
  <h2 class="text-[16px] font-semibold">外观</h2>

  <section class="mt-6">
    <p class="text-[11px] font-semibold tracking-wide text-ink-3 uppercase">主题</p>
    <div class="setting-row">
      <div>
        <p class="text-[12px] font-medium">界面主题</p>
        <p class="mt-0.5 text-[11px] text-ink-3">浅色为暖纸底色，深色参考 Obsidian</p>
      </div>
      <select class="setting-select" :value="theme" @change="emit('update-theme', ($event.target as HTMLSelectElement).value as ThemeId)">
        <option value="light">浅色</option>
        <option value="dark">深色</option>
      </select>
    </div>
  </section>

  <section class="mt-6">
    <p class="text-[11px] font-semibold tracking-wide text-ink-3 uppercase">正文</p>
    <div class="setting-row">
      <div>
        <p class="text-[12px] font-medium">正文字号</p>
        <p class="mt-0.5 text-[11px] text-ink-3">只影响笔记正文</p>
      </div>
      <select class="setting-select" :value="fontSize" @change="emit('update-font-size', ($event.target as HTMLSelectElement).value as FontSizeId)">
        <option v-for="option in fontSizeOptions" :key="option.id" :value="option.id">{{ option.label }}</option>
      </select>
    </div>
  </section>
</template>
