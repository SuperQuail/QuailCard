<!----------------------------------------------------------------
 通用设置：Vault（路径/最近列表/附件目录）、数据位置与复习偏好。
 全部动作上抛，组件不自持业务；主题与字号见 AppearanceSettings。
---------------------------------------------------------------->
<script setup lang="ts">
import { Clock3, Database, FolderOpen, FolderTree, Save } from "@lucide/vue";
import { computed, ref, watch } from "vue";
import type { DataLocations } from "../../domain/types";

const props = defineProps<{
  vaultPath: string;
  recentVaults: string[];
  aiGradingEnabled: boolean;
  attachmentFolder: string;
  attachmentFolderSaving: boolean;
  attachmentFolderStatus: string;
  dataLocations: DataLocations;
}>();

const emit = defineEmits<{
  "change-vault": [];
  "open-recent-vault": [path: string];
  "update-ai-grading": [enabled: boolean];
  "save-attachment-folder": [attachmentFolder: string];
  "reveal-data-folder": [target: "cards" | "config"];
}>();

const attachmentFolderDraft = ref(props.attachmentFolder);

/** Vault 配置变化时替换草稿，避免跨 Vault 串值。 */
watch(() => props.attachmentFolder, (value) => {
  attachmentFolderDraft.value = value;
});

/** 当前 Vault 之外的最近列表，点击即切换。 */
const otherRecents = computed(() =>
  props.recentVaults.filter((path) => path !== props.vaultPath).slice(0, 5),
);

/** 卡片数据目录的短展示名（vault 名 + .quailcard）。 */
const cardsDirLabel = computed(() => {
  const name = props.vaultPath.split(/[\\/]/).filter(Boolean).pop() ?? "";
  return name ? `${name}/.quailcard` : "未打开 Vault";
});
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
    <div v-if="otherRecents.length > 0" class="setting-row items-start">
      <div class="min-w-0 flex-1">
        <p class="text-[12px] font-medium">最近打开</p>
        <p class="mt-0.5 text-[11px] text-ink-3">点击直接切换到对应知识库</p>
        <div class="mt-2 space-y-1">
          <button
            v-for="path in otherRecents"
            :key="path"
            type="button"
            class="flex w-full items-center gap-2 rounded-lg border border-hairline bg-bg-paper px-3 py-1.5 text-left transition hover:border-accent"
            @click="emit('open-recent-vault', path)"
          >
            <Clock3 :size="12" class="shrink-0 text-ink-3" />
            <span class="min-w-0 truncate text-[11px] text-ink-2">{{ path }}</span>
          </button>
        </div>
      </div>
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
    <p class="text-[11px] font-semibold tracking-wide text-ink-3 uppercase">数据</p>
    <div class="setting-row">
      <div class="min-w-0">
        <p class="text-[12px] font-medium">卡片数据</p>
        <p class="mt-0.5 flex items-center gap-1.5 truncate text-[11px] text-ink-3" :title="dataLocations.cardsDir ?? undefined">
          <FolderTree :size="12" class="shrink-0" />{{ dataLocations.cardsDir ?? cardsDirLabel }}
        </p>
      </div>
      <button v-if="dataLocations.cardsDir" type="button" class="ghost-btn shrink-0 border border-hairline" @click="emit('reveal-data-folder', 'cards')">打开…</button>
    </div>
    <div class="setting-row">
      <div class="min-w-0">
        <p class="text-[12px] font-medium">应用配置</p>
        <p class="mt-0.5 text-[11px] text-ink-3">供应商、加密凭据与全局设置所在目录</p>
        <p class="mt-0.5 flex items-center gap-1.5 truncate text-[11px] text-ink-3" :title="dataLocations.configDir">
          <Database :size="12" class="shrink-0" />{{ dataLocations.configDir || "—" }}
        </p>
      </div>
      <button v-if="dataLocations.configDir" type="button" class="ghost-btn shrink-0 border border-hairline" @click="emit('reveal-data-folder', 'config')">打开…</button>
    </div>
  </section>

  <section class="mt-6">
    <p class="text-[11px] font-semibold tracking-wide text-ink-3 uppercase">复习</p>
    <div class="setting-row">
      <div>
        <p class="text-[12px] font-medium">使用问答时启用AI评分</p>
        <p class="mt-0.5 text-[11px] text-ink-3">关闭时问答卡自行对照答案评分</p>
      </div>
      <button type="button" role="switch" :aria-checked="aiGradingEnabled" class="toggle-switch" :class="{ 'is-on': aiGradingEnabled }" :aria-label="aiGradingEnabled ? '关闭 AI 评分' : '开启 AI 评分'" @click="emit('update-ai-grading', !aiGradingEnabled)"></button>
    </div>
  </section>
</template>
