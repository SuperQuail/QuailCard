<script setup lang="ts">
import { Check, LoaderCircle, LogIn, LogOut, Pencil, PlugZap, Plus, Trash2 } from "@lucide/vue";
import { onBeforeUnmount, ref } from "vue";
import { isTauri } from "../../api/backend";
import { resolveError } from "../../utils/errorMessage";
import { deleteProvider, getOpenAiLoginStatus, logoutOpenAi, startOpenAiLogin, testProvider } from "../../services/stores/providerStore";
import type { ProviderSummary } from "../../domain/types";
import ProviderForm from "./ProviderForm.vue";

/**
 * 模型设置：AI 供应商列表与 OpenAI 订阅登录。
 * 新增/编辑表单在 ProviderForm；活动供应商切换通过 emit 上抛由父层决定语义。
 */
defineProps<{
  providers: ProviderSummary[];
  activeProviderId: string;
}>();

const emit = defineEmits<{
  "set-active-provider": [providerId: string];
}>();

/** 表单状态：null 关闭，否则编辑目标（null id 表示新增）。 */
const formEditing = ref<ProviderSummary | null | undefined>(undefined);
const deletingProviderId = ref<string | null>(null);
const listMessage = ref("");
/** 正在测试的目标：供应商 ID，用于只让对应按钮显示加载态。 */
const busyKey = ref<string | null>(null);
let deleteTimer: number | undefined;

/** OAuth 登录状态。 */
const oauthBusy = ref(false);
const oauthMessage = ref("");
let oauthTimer: number | undefined;

/** 返回供应商状态文案。 */
function providerStatus(provider: ProviderSummary): string {
  if (provider.status === "connected") {
    return "已连接";
  }
  return provider.hasApiKey || provider.hasCredential ? "已配置" : "未测试";
}

/** 两段式删除：第一次点击进入确认，三秒内再点执行。 */
function requestDeleteProvider(providerId: string): void {
  if (deletingProviderId.value === providerId) {
    if (deleteTimer) {
      window.clearTimeout(deleteTimer);
    }
    deletingProviderId.value = null;
    listMessage.value = "";
    void deleteProvider(providerId).then(() => {
      listMessage.value = "已删除";
    }).catch((error) => {
      listMessage.value = resolveError(error);
    });
    return;
  }
  deletingProviderId.value = providerId;
  if (deleteTimer) {
    window.clearTimeout(deleteTimer);
  }
  deleteTimer = window.setTimeout(() => {
    deletingProviderId.value = null;
  }, 3000);
}

/** 使用已保存的供应商配置测试连接（行内快捷入口）。 */
async function testSavedProvider(provider: ProviderSummary): Promise<void> {
  if (busyKey.value !== null) {
    return;
  }
  busyKey.value = provider.id;
  listMessage.value = "";
  try {
    const result = await testProvider({
      id: provider.id,
      name: provider.name,
      shortCode: provider.shortCode,
      protocol: provider.protocol,
      model: provider.model,
      baseUrl: provider.baseUrl,
      supportsVision: provider.supportsVision,
      apiKey: null,
    });
    listMessage.value = `${provider.name} 连接正常 · ${result.latencyMs} ms`;
  } catch (error) {
    listMessage.value = resolveError(error);
  } finally {
    busyKey.value = null;
  }
}

/** 注销 OpenAI 订阅登录。 */
async function logoutSubscription(providerId: string): Promise<void> {
  await logoutOpenAi(providerId);
  oauthMessage.value = "已注销";
}

/** 启动 OpenAI 订阅登录并轮询状态。 */
async function startOauthLogin(): Promise<void> {
  if (oauthBusy.value) {
    return;
  }
  oauthBusy.value = true;
  oauthMessage.value = "正在启动浏览器登录…";
  try {
    const attempt = await startOpenAiLogin("openai_subscription", "browser");
    if (isTauri()) {
      const { openUrl } = await import("@tauri-apps/plugin-opener");
      await openUrl(attempt.url);
    } else {
      window.open(attempt.url, "_blank");
    }
    oauthTimer = window.setInterval(async () => {
      try {
        const status = await getOpenAiLoginStatus(attempt.attemptId);
        if (status.status === "success") {
          window.clearInterval(oauthTimer);
          oauthBusy.value = false;
          oauthMessage.value = "登录成功";
          return;
        }
        if (status.status === "failed" || status.status === "cancelled") {
          window.clearInterval(oauthTimer);
          oauthBusy.value = false;
          oauthMessage.value = status.message || "登录失败";
          return;
        }
        oauthMessage.value = status.message || "等待浏览器授权…";
      } catch (error) {
        window.clearInterval(oauthTimer);
        oauthBusy.value = false;
        oauthMessage.value = resolveError(error);
      }
    }, 2000);
  } catch (error) {
    oauthBusy.value = false;
    oauthMessage.value = resolveError(error);
  }
}

onBeforeUnmount(() => {
  if (oauthTimer) {
    window.clearInterval(oauthTimer);
  }
});
</script>

<template>
  <h2 class="text-[16px] font-semibold">模型</h2>
  <section class="mt-6">
    <p class="text-[11px] font-semibold tracking-wide text-ink-3 uppercase">供应商</p>
    <div v-for="provider in providers" :key="provider.id" class="setting-row">
      <button type="button" class="flex min-w-0 flex-1 items-center gap-2.5 text-left" title="设为活动供应商" @click="emit('set-active-provider', provider.id)">
        <span class="grid size-6 shrink-0 place-items-center rounded-full border border-hairline text-[9px] font-bold">{{ provider.name.slice(0, 1) }}</span>
        <span class="min-w-0">
          <span class="flex items-center gap-1.5 truncate text-[12px] font-medium">
            {{ provider.name }} · {{ provider.model }}
            <Check v-if="activeProviderId === provider.id" :size="13" class="shrink-0 text-accent-strong" />
          </span>
          <span class="block truncate text-[10px] text-ink-3">{{ provider.protocol }} · {{ providerStatus(provider) }}</span>
        </span>
      </button>
      <span class="flex shrink-0 items-center gap-0.5">
        <button type="button" class="icon-btn" title="测试连接" :disabled="busyKey !== null" @click="testSavedProvider(provider)">
          <LoaderCircle v-if="busyKey === provider.id" :size="14" class="animate-spin" />
          <PlugZap v-else :size="14" />
        </button>
        <button v-if="provider.providerType === 'openai_subscription' && !provider.hasCredential" type="button" class="icon-btn" title="登录" :disabled="oauthBusy" @click="startOauthLogin">
          <LogIn :size="14" />
        </button>
        <button v-else-if="provider.providerType === 'openai_subscription'" type="button" class="icon-btn" title="注销" :disabled="oauthBusy" @click="void logoutSubscription(provider.id)">
          <LogOut :size="14" />
        </button>
        <button type="button" class="icon-btn" title="编辑" @click="formEditing = provider">
          <Pencil :size="14" />
        </button>
        <button
          type="button"
          class="icon-btn"
          :class="deletingProviderId === provider.id ? '!text-danger' : 'hover:!text-danger'"
          :title="deletingProviderId === provider.id ? '再点一次确认删除' : '删除'"
          @click="requestDeleteProvider(provider.id)"
        >
          <Trash2 :size="14" />
        </button>
      </span>
    </div>
    <p v-if="listMessage" class="mt-2 text-[11px] text-success">{{ listMessage }}</p>
    <p v-if="oauthMessage" class="mt-1 text-[11px] text-ink-2">{{ oauthMessage }}</p>

    <!-- 新增 / 编辑表单 -->
    <ProviderForm v-if="formEditing !== undefined" :editing="formEditing" @close="formEditing = undefined" />
    <button v-else type="button" class="ghost-btn mt-3 border border-hairline" @click="formEditing = null">
      <Plus :size="13" />新增供应商
    </button>
  </section>
</template>
