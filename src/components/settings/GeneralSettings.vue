<script setup lang="ts">
import { FolderOpen, Moon, Save, Sun } from "@lucide/vue";
import { ref, watch } from "vue";
import type { FontSizeId, ThemeId } from "../../domain/types";

/** 通用设置：Vault 路径、外观（主题/字号）与复习偏好。全部动作上抛，组件不自持业务。 */
const props = defineProps<{
  theme: ThemeId;
  fontSize: FontSizeId;
  vaultPath: string;
  aiGradingEnabled: boolean;
  attachmentFolder: string;
  attachmentFolderSaving: boolean;
  attachmentFolderStatus: string;
}>();

const emit = defineEmits<{
  "update-theme": [theme: ThemeId];
  "update-font-size": [size: FontSizeId];
  "change-vault": [];
  "update-ai-grading": [enabled: boolean];
  "save-attachment-folder": [attachmentFolder: string];
}>();

const attachmentFolderDraft = ref(props.attachmentFolder);

/** Vault 配置变化时替换草稿，避免跨 Vault 串值。 */
watch(() => props.attachmentFolder, (value) => {
  attachmentFolderDraft.value = value;
});

/** 字号档位选项（紧凑/标准/舒适）。 */
const fontSizeOptions: Array<{ id: FontSizeId; label: string }> = [
  { id: "compact", label: "紧凑" },
  { id: "standard", label: "标准" },
  { id: "comfortable", label: "舒适" },
];
</script>

<template>
  <h2 class="text-[16px] font-semibold">通用</h2>

  <section class="mt-6">
    <p class="text-[11px] font-semibold tracking-wide text-ink-3 uppercase">Vault</p>
    <div class="setting-row">
      <div class="min-w-0">
        <p class="text-[12px] font-medium">Vault 文件夹</p>
        <p class="mt-0.5 flex items-center gap-1.5 truncate text-[11px] text-ink-3">
          <FolderOpen :size="12" class="shrink-0" />{{ vaultPath || "未选择" }}
        </p>
      </div>
      <button type="button" class="ghost-btn shrink-0 border border-hairline" @click="emit('change-vault')">更换…</button>
    </div>
    <div class="setting-row items-start">
      <div class="min-w-0 flex-1">
        <label for="attachment-folder" class="text-[12px] font-medium">图片附件文件夹</label>
        <p class="mt-0.5 text-[11px] text-ink-3">相对于当前 Vault，粘贴和拖入的图片将保存到这里</p>
        <div class="mt-2 flex items-center gap-2">
          <input id="attachment-folder" v-model="attachmentFolderDraft" class="field-input" placeholder="attachments" @keydown.enter="emit('save-attachment-folder', attachmentFolderDraft)" />
          <button type="button" class="primary-btn shrink-0" :disabled="attachmentFolderSaving || !attachmentFolderDraft.trim()" @click="emit('save-attachment-folder', attachmentFolderDraft)">
            <Save :size="12" />{{ attachmentFolderSaving ? "保存中" : "保存" }}
          </button>
        </div>
        <p v-if="attachmentFolderStatus" class="mt-1.5 text-[11px]" :class="attachmentFolderStatus === '已保存' ? 'text-success' : 'text-danger'">{{ attachmentFolderStatus }}</p>
      </div>
    </div>
  </section>

  <section class="mt-6">
    <p class="text-[11px] font-semibold tracking-wide text-ink-3 uppercase">外观</p>
    <div class="setting-row">
      <div>
        <p class="text-[12px] font-medium">主题</p>
        <p class="mt-0.5 text-[11px] text-ink-3">浅色为暖纸底色，深色参考 Obsidian</p>
      </div>
      <div class="flex shrink-0 rounded-lg bg-bg-side p-0.5">
        <button type="button" class="flex items-center gap-1.5 rounded-md px-3 py-1.5 text-[11px] font-medium transition" :class="theme === 'light' ? 'bg-bg-paper text-accent-strong shadow-sm' : 'text-ink-3 hover:text-ink-2'" @click="emit('update-theme', 'light')">
          <Sun :size="12" />浅色
        </button>
        <button type="button" class="flex items-center gap-1.5 rounded-md px-3 py-1.5 text-[11px] font-medium transition" :class="theme === 'dark' ? 'bg-bg-paper text-accent-strong shadow-sm' : 'text-ink-3 hover:text-ink-2'" @click="emit('update-theme', 'dark')">
          <Moon :size="12" />深色
        </button>
      </div>
    </div>
    <div class="setting-row">
      <div>
        <p class="text-[12px] font-medium">正文字号</p>
        <p class="mt-0.5 text-[11px] text-ink-3">只影响笔记正文</p>
      </div>
      <div class="flex shrink-0 rounded-lg bg-bg-side p-0.5">
        <button v-for="option in fontSizeOptions" :key="option.id" type="button" class="rounded-md px-3 py-1.5 text-[11px] font-medium transition" :class="fontSize === option.id ? 'bg-bg-paper text-accent-strong shadow-sm' : 'text-ink-3 hover:text-ink-2'" @click="emit('update-font-size', option.id)">
          {{ option.label }}
        </button>
      </div>
    </div>
  </section>

  <section class="mt-6">
    <p class="text-[11px] font-semibold tracking-wide text-ink-3 uppercase">复习</p>
    <div class="setting-row">
      <div>
        <p class="text-[12px] font-medium">使用问答时启用AI评分</p>
        <p class="mt-0.5 text-[11px] text-ink-3">关闭时问答卡自行对照答案评分</p>
      </div>
      <div class="flex shrink-0 rounded-lg bg-bg-side p-0.5">
        <button type="button" class="rounded-md px-3 py-1.5 text-[11px] font-medium transition" :class="aiGradingEnabled ? 'bg-bg-paper text-accent-strong shadow-sm' : 'text-ink-3 hover:text-ink-2'" @click="emit('update-ai-grading', true)">开启</button>
        <button type="button" class="rounded-md px-3 py-1.5 text-[11px] font-medium transition" :class="!aiGradingEnabled ? 'bg-bg-paper text-accent-strong shadow-sm' : 'text-ink-3 hover:text-ink-2'" @click="emit('update-ai-grading', false)">关闭</button>
      </div>
    </div>
  </section>
</template>
