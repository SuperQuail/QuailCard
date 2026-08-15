<script setup lang="ts">
import { KeyRound, Monitor, Server, X } from "@lucide/vue";
import { ref } from "vue";
import GeneralSettings from "./settings/GeneralSettings.vue";
import ModelSettings from "./settings/ModelSettings.vue";
import VaultSecuritySettings from "./settings/VaultSecuritySettings.vue";
import type { FontSizeId, ProviderSummary, ThemeId, VaultStatus } from "../domain/types";

/** 设置覆盖层：只负责 tab 导航与三个设置面板的装配，业务全部在子组件与 store。 */
defineProps<{
  theme: ThemeId;
  fontSize: FontSizeId;
  vaultPath: string;
  providers: ProviderSummary[];
  activeProviderId: string;
  vaultStatus: VaultStatus | null;
  aiGradingEnabled: boolean;
}>();

const emit = defineEmits<{
  close: [];
  "update-theme": [theme: ThemeId];
  "update-font-size": [size: FontSizeId];
  "change-vault": [];
  "set-active-provider": [providerId: string];
  "set-vault-password": [password: string];
  "update-ai-grading": [enabled: boolean];
}>();

type TabId = "general" | "model" | "vault";
const tab = ref<TabId>("general");

/** 设置类别定义：导航渲染与 tab 切换共用。 */
const tabs: Array<{ id: TabId; label: string; icon: typeof Monitor }> = [
  { id: "general", label: "通用", icon: Monitor },
  { id: "model", label: "模型", icon: Server },
  { id: "vault", label: "安全", icon: KeyRound },
];
</script>

<template>
  <!-- 设置：占满窗口的视图，左侧类别栏 + 右侧内容列（Obsidian 形态） -->
  <div class="fixed inset-0 z-70 flex flex-col bg-bg">
    <header class="flex h-11 shrink-0 items-center justify-between border-b border-hairline pr-2 pl-5">
      <h1 class="text-[13px] font-semibold">设置</h1>
      <button type="button" class="icon-btn" title="关闭设置" @click="emit('close')">
        <X :size="16" />
      </button>
    </header>

    <div class="flex min-h-0 flex-1">
      <!-- 类别栏 -->
      <nav class="w-[188px] shrink-0 space-y-0.5 overflow-y-auto border-r border-hairline bg-bg-side p-2" aria-label="设置类别">
        <button
          v-for="item in tabs"
          :key="item.id"
          type="button"
          class="flex w-full items-center gap-2.5 rounded-md px-2.5 py-2 text-left text-[12px] transition"
          :class="tab === item.id ? 'bg-bg-active font-medium text-accent-strong' : 'text-ink-2 hover:bg-bg-hover'"
          @click="tab = item.id"
        >
          <component :is="item.icon" :size="14" :stroke-width="1.8" />{{ item.label }}
        </button>
      </nav>

      <!-- 内容列 -->
      <div class="soft-scrollbar min-w-0 flex-1 overflow-y-auto">
        <div class="mx-auto w-full max-w-[640px] px-8 pt-6 pb-16">
          <GeneralSettings
            v-if="tab === 'general'"
            :theme="theme"
            :font-size="fontSize"
            :vault-path="vaultPath"
            :ai-grading-enabled="aiGradingEnabled"
            @update-theme="emit('update-theme', $event)"
            @update-font-size="emit('update-font-size', $event)"
            @change-vault="emit('change-vault')"
            @update-ai-grading="emit('update-ai-grading', $event)"
          />
          <ModelSettings
            v-else-if="tab === 'model'"
            :providers="providers"
            :active-provider-id="activeProviderId"
            @set-active-provider="emit('set-active-provider', $event)"
          />
          <VaultSecuritySettings
            v-else
            :vault-status="vaultStatus"
            @set-vault-password="emit('set-vault-password', $event)"
          />
        </div>
      </div>
    </div>
  </div>
</template>
